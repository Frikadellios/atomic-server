//! Store - this is an in-memory store of Atomic data.
//! This provides many methods for finding, changing, serializing and parsing Atomic Data.
//! Currently, it can only persist its data as .ad3 (Atomic Data Triples) to disk.
//! A more robust persistent storage option will be used later, such as: https://github.com/TheNeikos/rustbreak

use crate::errors::AtomicResult;
use crate::mutations;
use crate::values::Value;
use crate::{
    atoms::Atom,
    storelike::{ResourceString, Storelike, ResourceCollection},
};
use std::{collections::HashMap, fs, path::PathBuf};

/// The in-memory store of data, containing the Resources, Properties and Classes
#[derive(Clone)]
pub struct Store {
    // The store currently holds two stores - that is not ideal
    hashmap: HashMap<String, ResourceString>,
    log: mutations::Log,
}

impl Store {
    /// Create an empty Store. This is where you start.
    ///
    /// # Example
    /// let store = Store::init();
    pub fn init() -> Store {
        return Store {
            hashmap: HashMap::new(),
            log: Vec::new(),
        };
    }

    /// Reads an .ad3 (Atomic Data Triples) graph and adds it to the store
    pub fn read_store_from_file<'a>(&mut self, path: &'a PathBuf) -> AtomicResult<()> {
        match std::fs::read_to_string(path) {
            Ok(contents) => {
                self.parse_ad3(&contents)?;
                Ok(())
            }
            Err(err) => Err(format!("Parsing error: {}", err).into()),
        }
    }

    /// Serializes the current store and saves to path
    pub fn write_store_to_disk(&self, path: &PathBuf) -> AtomicResult<()> {
        let mut file_string: String = String::new();
        for (subject, _) in self.all_resources()? {
            let resourcestring = self.resource_to_ad3(&subject, None)?;
            &file_string.push_str(&*resourcestring);
        }
        fs::create_dir_all(path.parent().expect("Could not find parent folder"))
            .expect("Unable to create dirs");
        fs::write(path, file_string).expect("Unable to write file");
        return Ok(());
    }

    /// Checks Atomic Data in the store for validity.
    /// Returns an Error if it is not valid.
    ///
    /// Validates:
    ///
    /// - [X] If the Values can be parsed using their Datatype (e.g. if Integers are integers)
    /// - [X] If all required fields of the class are present
    /// - [ ] If the URLs are publicly accessible and return the right type of data
    /// - [ ] Returns a report with multiple options
    #[allow(dead_code, unreachable_code)]
    pub fn validate_store(&self) -> AtomicResult<()> {
        for (subject, resource) in self.all_resources()? {
            println!("Subject: {:?}", subject);
            println!("Resource: {:?}", resource);

            let mut found_props: Vec<String> = Vec::new();

            for (prop_url, value) in resource {
                let property = self.get_property(&prop_url)?;

                Value::new(&value, &property.data_type)?;
                found_props.push(prop_url.clone());
                // println!("{:?}: {:?}", prop_url, value);
            }
            let classes = self.get_classes_for_subject(&subject)?;
            for class in classes {
                println!("Class: {:?}", class.shortname);
                println!("Found: {:?}", found_props);
                for required_prop in class.requires {
                    println!("Required: {:?}", required_prop.shortname);
                    if !found_props.contains(&required_prop.subject) {
                        return Err(format!(
                            "Missing requried property {} in {} because of class {}",
                            &required_prop.shortname, subject, class.subject,
                        )
                        .into());
                    }
                }
            }
            println!("{:?} Valid", subject);
        }
        return Ok(());
    }

    /// Loads the default Atomic Store, containing the Properties, Datatypes and Clasess for Atomic Schema.
    pub fn load_default(&mut self) {
        let ad3 = include_str!("../../defaults/default_store.ad3");
        self.parse_ad3(&String::from(ad3)).unwrap();
    }
}

impl Storelike for Store {
    fn add_atoms(&mut self, atoms: Vec<Atom>) -> AtomicResult<()> {
        for atom in atoms {
            match self.hashmap.get_mut(&atom.subject) {
                Some(resource) => {
                    resource.insert(atom.property, atom.value);
                }
                None => {
                    let mut resource: ResourceString = HashMap::new();
                    resource.insert(atom.property, atom.value);
                    self.hashmap.insert(atom.subject, resource);
                }
            }
        }
        return Ok(());
    }

    fn add_resource_string(&mut self, subject: String, resource: &ResourceString) -> AtomicResult<()> {
        self.hashmap.insert(subject, resource.clone());
        return Ok(());
    }

    fn all_resources(&self) -> AtomicResult<ResourceCollection> {
        let res = self.hashmap.clone().into_iter().collect();
        Ok(res)
    }

    fn get_resource_string(&self, resource_url: &String) -> Option<ResourceString> {
        match self.hashmap.get(resource_url) {
            Some(result) => Some(result.clone()),
            None => None,
        }
    }
}


#[cfg(test)]
mod test {
    use super::*;
    use crate::urls;

    fn init_store() -> Store {
        let string =
            String::from("[\"_:test\",\"https://atomicdata.dev/properties/shortname\",\"hi\"]");
        let mut store = Store::init();
        store.load_default();
        // Run parse...
        store.parse_ad3(&string).unwrap();
        return store;
    }

    #[test]
    fn get() {
        let store = init_store();
        // Get our resource...
        let my_resource = store.get_resource_string(&"_:test".into()).unwrap();
        // Get our value by filtering on our property...
        let my_value = my_resource
            .get("https://atomicdata.dev/properties/shortname")
            .unwrap();
        println!("My value: {}", my_value);
        assert!(my_value == "hi");
    }

    #[test]
    fn validate() {
        let store = init_store();
        store.validate_store().unwrap();
    }

    #[test]
    #[should_panic]
    fn validate_invalid() {
        let mut store = init_store();
        let invalid_ad3 =
            // should be array, is string
            String::from("[\"_:test\",\"https://atomicdata.dev/properties/requires\",\"Test\"]");
        store.parse_ad3(&invalid_ad3).unwrap();
        store.validate_store().unwrap();
    }

    #[test]
    fn serialize() {
        let store = init_store();
        store
            .resource_to_json(&String::from(urls::CLASS), 1, true)
            .unwrap();
    }
}