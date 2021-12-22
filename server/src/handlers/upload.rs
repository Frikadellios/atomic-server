use std::{ffi::OsStr, path::Path, sync::Mutex};

use actix_multipart::Multipart;
use actix_web::{web, HttpResponse};
use async_std::prelude::*;
use atomic_lib::{
    commit::CommitResponse, datetime_helpers::now, hierarchy::check_write, urls, AtomicError,
    Resource, Storelike, Value,
};
use futures::{StreamExt, TryStreamExt};
use serde::Deserialize;

use crate::{appstate::AppState, errors::AtomicServerResult, helpers::get_client_agent};

#[derive(Deserialize)]
pub struct UploadQuery {
    parent: String,
}

/// Allows the user to upload files tot the `/upload` endpoint.
/// A parent Query parameter is required for checking rights and for placing the file in a Hierarchy.
/// Creates new File resources for every submitted file.
/// Submission is done using multipart/form-data.
/// The file is stored in the `/uploads` directory.
/// An `attachment` relationship is created from the parent
pub async fn upload_handler(
    mut body: Multipart,
    data: web::Data<Mutex<AppState>>,
    query: web::Query<UploadQuery>,
    req: actix_web::HttpRequest,
) -> AtomicServerResult<HttpResponse> {
    let appstate = data.lock().unwrap();
    let store = &appstate.store;
    let parent = store.get_resource(&query.parent)?;
    let subject = format!(
        "{}{}",
        store.get_base_url(),
        req.head()
            .uri
            .path_and_query()
            .ok_or("Path must be given")?
    );
    if let Some(agent) = get_client_agent(req.headers(), &appstate, subject)? {
        check_write(store, &parent, &agent)?;
    } else {
        return Err(AtomicError::unauthorized(
            "No authorization headers present. These are required when uploading files.".into(),
        )
        .into());
    }

    let mut created_resources: Vec<Resource> = Vec::new();
    let mut commit_responses: Vec<CommitResponse> = Vec::new();

    while let Ok(Some(mut field)) = body.try_next().await {
        let content_type = field
            .content_disposition()
            .ok_or("actix_web::error::ParseError::Incomplete")?;
        let filename = content_type.get_filename().ok_or("Filename is missing")?;

        let filesdir = format!("{}/uploads", appstate.config.config_dir.to_str().unwrap());
        async_std::fs::create_dir_all(&filesdir).await?;

        let file_id = format!(
            "{}-{}",
            now(),
            sanitize_filename::sanitize(&filename)
                // Spacebars lead to very annoying bugs in browsers
                .replace(" ", "-")
        );
        let file_path = format!("{}/{}", filesdir, file_id);
        let mut file = async_std::fs::File::create(file_path).await?;

        // Field in turn is stream of *Bytes* object
        while let Some(chunk) = field.next().await {
            let data = chunk.unwrap();
            // TODO: Update a SHA256 hash here for checksum
            file.write_all(&data).await?;
        }

        let byte_count: i64 = file
            .metadata()
            .await?
            .len()
            .try_into()
            .map_err(|_e| "Too large")?;

        let subject_path = format!("files/{}", urlencoding::encode(&file_id));
        let new_subject = format!("{}/{}", store.get_base_url(), subject_path);
        let download_url = format!("{}/download/{}", store.get_base_url(), subject_path);

        let mut resource = atomic_lib::Resource::new_instance(urls::FILE, store)?;
        resource.set_subject(new_subject);
        resource.set_propval_string(urls::PARENT.into(), &query.parent, store)?;
        resource.set_propval_string(urls::INTERNAL_ID.into(), &file_id, store)?;
        resource.set_propval(urls::FILESIZE.into(), Value::Integer(byte_count), store)?;
        resource.set_propval_string(
            urls::MIMETYPE.into(),
            &guess_mime_for_filename(filename),
            store,
        )?;
        resource.set_propval_string(urls::FILENAME.into(), filename, store)?;
        resource.set_propval_string(urls::DOWNLOAD_URL.into(), &download_url, store)?;
        commit_responses.push(resource.save(store)?);
        created_resources.push(resource);
    }

    let created_file_subjects = created_resources
        .iter()
        .map(|r| r.get_subject().to_string())
        .collect::<Vec<String>>();

    // Add the files as `attachments` to the parent
    let mut parent = store.get_resource(&query.parent)?;
    parent.append_subjects(urls::ATTACHMENTS, created_file_subjects, false, store)?;
    commit_responses.push(parent.save(store)?);

    for resp in commit_responses {
        // When a commit is applied, notify all webhook subscribers
        // TODO: add commit handler https://github.com/joepio/atomic-data-rust/issues/253
        appstate
            .commit_monitor
            .do_send(crate::actor_messages::CommitMessage {
                commit_response: resp,
            });
    }

    let mut builder = HttpResponse::Ok();

    Ok(builder.body(atomic_lib::serialize::resources_to_json_ad(
        &created_resources,
    )?))
}

fn guess_mime_for_filename(filename: &str) -> String {
    if let Some(ext) = get_extension_from_filename(filename) {
        actix_files::file_extension_to_mime(ext).to_string()
    } else {
        "application/octet-stream".to_string()
    }
}

fn get_extension_from_filename(filename: &str) -> Option<&str> {
    Path::new(filename).extension().and_then(OsStr::to_str)
}