/*!
 Contains logic to parse detailed data from `typedstream` data, focussing specifically on [NSAttributedString](https://developer.apple.com/documentation/foundation/nsattributedstring) data.

 Derived from `typedstream` source located [here](https://opensource.apple.com/source/gcc/gcc-1493/libobjc/objc/typedstream.h.auto.html) and [here](https://sourceforge.net/projects/aapl-darwin/files/Darwin-0.1/objc-1.tar.gz/download)
*/

/// Indicates the start of a new object
const START: u8 = 0x0084;
/// No data to parse, possibly end of an inheritance chain
const EMPTY: u8 = 0x0085;
/// Indicates the last byte of an object
const END: u8 = 0x0086;

/// Type encoding data
const ENCODING_DETECTED: u8 = 0x0095;

/// When scanning for objects, bytes >= reference tag indicate an index in the table of
/// already-seen types
const REFERENCE_TAG: u8 = 0x0092;

#[derive(Debug, Clone)]
struct Class {
    name: String,
    version: u8,
}

impl Class {
    fn new(name: String, version: u8) -> Self {
        Self { name, version }
    }

    fn as_string(&self) -> String {
        return format!("{} v{}", self.name, self.version);
    }
}

#[derive(Debug, Clone)]
enum OutputData {
    String(String),
    Number(i32),
    Byte(u8),
    Class(Class),
    NewObject,
    Reference(u8),
    Placeholder,
    None,
}

#[derive(Debug, Clone)]
enum Archivable {
    Object(Vec<OutputData>),
    Class(Class),
}

// TODO: Remove clone
#[derive(Debug, Clone)]
enum Type {
    Utf8String,
    EmbeddedData,
    Object,
    SignedInt,
    UnsignedInt,
    String(String),
    Unknown(u8),
}

impl Type {
    fn from_byte(byte: &u8) -> Self {
        match byte {
            0x0040 => Self::Object,
            0x002B => Self::Utf8String,
            0x002A => Self::EmbeddedData,
            0x0069 => Self::UnsignedInt,
            0x0049 => Self::SignedInt,
            other => Self::Unknown(*other),
        }
    }

    fn new_string(string: String) -> Self {
        Self::String(string)
    }
}

#[derive(Debug)]
struct TypedStreamReader<'a> {
    stream: &'a [u8],
    idx: usize,
    types_table: Vec<Vec<Type>>,
    object_table: Vec<Archivable>,
}

impl<'a> TypedStreamReader<'a> {
    fn new(stream: &'a [u8]) -> Self {
        Self {
            stream,
            idx: 0,
            types_table: vec![],
            object_table: vec![],
        }
    }

    // TODO: Remove
    fn emit_objects_table(&self) {
        println!("Start types table");
        self.types_table
            .iter()
            .enumerate()
            .for_each(|(idx, types)| println!("\t{idx}: {types:?}"));
        println!("End types table");
        println!("Start objects table");
        self.object_table
            .iter()
            .enumerate()
            .for_each(|(idx, obj)| println!("\t{idx}: {obj:?}"));
        println!("End objects table");
    }

    // TODO: Remove
    fn print_loc(&self, name: &str) {
        println!("{name}: {:x}: {:x}", self.idx, self.get_current_byte());
    }

    /// Read the current byte as a signed integer
    fn read_int(&mut self) -> u8 {
        let value = u8::from_le_bytes([self.get_current_byte()]);
        self.idx += 1;
        value
    }

    /// Read exactly `n` bytes from the stream
    fn read_exact_bytes(&mut self, n: usize) -> &[u8] {
        let range = &self.stream[self.idx..self.idx + n];
        self.idx += n;
        range
    }

    /// Read `n` bytes as a String
    fn read_exact_as_string(&mut self, n: usize, string: &mut String) {
        let str = std::str::from_utf8(self.read_exact_bytes(n)).unwrap();
        string.push_str(str);
    }

    /// Read the current byte
    fn get_current_byte(&self) -> u8 {
        self.stream[self.idx]
    }

    /// Read the next byte
    // TODO: Bounds check
    fn get_next_byte(&self) -> u8 {
        self.stream[self.idx + 1]
    }

    /// Determine the current types
    fn read_type(&mut self) -> Vec<Type> {
        let length = self.read_int();
        println!("type length: {length}");
        self.read_exact_bytes(length as usize)
            .iter()
            .map(Type::from_byte)
            .collect()
    }

    /// Read a reference pointer for a Type
    fn read_pointer(&mut self) -> u8 {
        self.print_loc("pointer");
        let result = self.get_current_byte() - REFERENCE_TAG;
        self.idx += 1;
        result
    }

    /// Read a class
    fn read_class(&mut self) -> Option<&Archivable> {
        match self.get_current_byte() {
            START => {
                // Skip some header bytes
                self.print_loc("class 1");
                while self.get_current_byte() == START {
                    self.idx += 1;
                }
                self.print_loc("class 2");
                let length = self.read_int();
                if length >= REFERENCE_TAG {
                    let index = length - REFERENCE_TAG;
                    // TODO: this is a reference to a string, we should build a class with that name
                    // Or store the class as the Type
                    println!("Getting referenced class at {index}");
                    return self.object_table.get(index as usize);
                }
                let mut class_name = String::with_capacity(length as usize);
                println!("Class name created with capacity {}", class_name.capacity());
                self.read_exact_as_string(length as usize, &mut class_name);

                let version = self.read_int();
                println!("{class_name} v{version}");
                println!("{}: {:?}", self.idx, self.get_current_byte());

                self.types_table
                    .push(vec![Type::new_string(class_name.clone())]);

                self.object_table
                    .push(Archivable::Class(Class::new(class_name, version)));

                self.read_class()?;
                self.object_table.last()
            }
            EMPTY => {
                self.idx += 1;
                println!("End of class chain!");
                self.object_table.last()
            }
            ENCODING_DETECTED => {
                let embedded_data = self.read_embedded_data();
                self.object_table.push(Archivable::Object(embedded_data));
                self.object_table.last()
            }
            _ => {
                let index = self.read_pointer();
                println!("Getting referenced object at {index}");
                self.object_table.get(index as usize)
            }
        }
    }

    /// read an object
    fn read_object(&mut self) -> Option<&Archivable> {
        match self.get_current_byte() {
            START => {
                if let Some(obj_class) = self.read_class() {
                    return Some(obj_class);
                }
                println!("Failed to read class!");
                None
            }
            EMPTY => {
                self.idx += 1;
                println!("Got empty object!");
                None
            }
            _ => {
                let index = self.read_pointer();
                self.object_table.get(index as usize)
            }
        }
    }

    /// Read String data
    fn read_string(&mut self) -> String {
        let length = self.read_int();
        let mut string = String::with_capacity(length as usize);
        println!("String created with capacity {}", string.capacity());
        self.read_exact_as_string(length as usize, &mut string);

        string
    }

    fn read_embedded_data(&mut self) -> Vec<OutputData> {
        // Skip the 0x84
        self.idx += 1;
        let parsed_type = self.get_type();
        self.read_types(parsed_type)
    }

    fn get_type(&mut self) -> Vec<Type> {
        match self.get_current_byte() {
            START => {
                println!("New type found!");
                // Ignore repeated types, for example in a dict
                self.idx += 1;
                let object_types = self.read_type();
                self.types_table.push(object_types);
                println!("Found types: {:?}", self.types_table);
                self.types_table.last().unwrap().to_owned()
            }
            END => {
                // TODO: This doesn't make any sense, we should have a Result<> or Option<> here
                println!("End of current object!");
                vec![]
            }
            _ => {
                // Ignore repeated types, for example in a dict
                while self.get_current_byte() == self.get_next_byte() {
                    self.idx += 1;
                }

                let ref_tag = self.read_pointer();
                let possible_types = self.types_table.get(ref_tag as usize).unwrap().clone();
                println!("Got referenced type {ref_tag}: {possible_types:?}");
                possible_types
            }
        }
    }

    fn read_types(&mut self, found_types: Vec<Type>) -> Vec<OutputData> {
        let mut out_v = vec![];
        // Objects reserve their place in the table when they begin in the stream, not where there data is
        // Which may be buried under an inheritance chain
        // let mut placeholder: Option<usize> = None;
        for object_type in found_types {
            match object_type {
                Type::Utf8String => out_v.push(OutputData::String(self.read_string())),
                Type::EmbeddedData => out_v.extend(self.read_embedded_data()),
                Type::Object => {
                    println!("Reading object...");
                    self.print_loc("reading object at");
                    let object = self.read_object();
                    println!("Got object {object:?}");
                    if let Some(object) = object {
                        match object.clone() {
                            Archivable::Object(data) => out_v.extend(data),
                            Archivable::Class(cls) => out_v.push(OutputData::Class(cls)),
                        }
                    } else {
                        out_v.push(OutputData::None)
                    }
                }
                Type::SignedInt => out_v.push(OutputData::Number(self.read_int() as i32)),
                Type::UnsignedInt => out_v.push(OutputData::Number(self.read_int() as i32)),
                Type::Unknown(byte) => out_v.push(OutputData::Byte(byte)),
                Type::String(s) => out_v.push(OutputData::String(s)),
            };
            continue;
        }
        // if let Some(place) = placeholder {
        //     println!("Inserting {out_v:?} to object table at {place}");
        //     self.object_table[place] = Archivable::Object(out_v.clone())
        // }
        out_v
    }

    /// Attempt to get the data from the typed stream
    fn parse(&mut self) -> Vec<Vec<OutputData>> {
        let mut out_v = vec![];

        // Skip header
        // TODO: Parse it
        self.idx += 16;

        while self.idx < self.stream.len() {
            if self.get_current_byte() == END {
                println!("End of object!");
                self.idx += 1;
                continue;
            }

            println!("Parsed data: {:?}\n", out_v);

            // First, get the current type
            let found_types = self.get_type();
            println!("Received types: {:?}", found_types);

            let result = self.read_types(found_types);
            println!("Resultant type: {result:?}");

            out_v.push(result);
            self.emit_objects_table();
            println!("Types table: {:?}", self.types_table);
        }

        self.emit_objects_table();
        println!("Types table: {:?}", self.types_table);
        println!("Parsed data: {:?}\n", out_v);
        out_v
    }
}

#[cfg(test)]
mod tests {
    use std::env::current_dir;
    use std::fs::File;
    use std::io::Read;
    use std::vec;

    use crate::util::attributed_string::{Archivable, Class, TypedStreamReader};

    #[test]
    fn test_parse_text_mention() {
        let plist_path = current_dir()
            .unwrap()
            .as_path()
            .join("test_data/mentions/Mention");
        let mut file = File::open(plist_path).unwrap();
        let mut bytes = vec![];
        file.read_to_end(&mut bytes).unwrap();

        let mut parser = TypedStreamReader::new(&bytes);
        // parser.object_table = vec![
        //     Archivable::Object,
        //     Archivable::Class(Class {
        //         name: "NSMutableAttributedString".to_string(),
        //         version: 0,
        //     }),
        //     Archivable::Class(Class {
        //         name: "NSAttributedString".to_string(),
        //         version: 0,
        //     }),
        //     Archivable::Class(Class {
        //         name: "NSObject".to_string(),
        //         version: 0,
        //     }),
        //     Archivable::Object, // body text
        //     Archivable::Class(Class {
        //         name: "NSMutableString".to_string(),
        //         version: 1,
        //     }),
        //     Archivable::Class(Class {
        //         name: "NSString".to_string(),
        //         version: 1,
        //     }),
        //     Archivable::Object, // key name
        //     Archivable::Class(Class {
        //         name: "NSDictionary".to_string(),
        //         version: 0,
        //     }),
        //     Archivable::Object, // value data
        //     Archivable::Object, // unknown
        //     Archivable::Class(Class {
        //         name: "NSNumber".to_string(),
        //         version: 0,
        //     }),
        //     Archivable::Class(Class {
        //         name: "NSValue".to_string(),
        //         version: 0,
        //     }),
        // ];
        println!("{parser:?}");
        let result = parser.parse();

        println!("\n\nGot data!");
        result.iter().for_each(|item| println!("\n{item:?}"))

        // let expected = "Noter test".to_string();

        // assert_eq!(parsed, expected);
    }

    #[test]
    fn test_parse_text_basic() {
        let plist_path = current_dir()
            .unwrap()
            .as_path()
            .join("test_data/streamtyped/AttributedBodyTextOnly");
        let mut file = File::open(plist_path).unwrap();
        let mut bytes = vec![];
        file.read_to_end(&mut bytes).unwrap();

        let mut parser = TypedStreamReader::new(&bytes);
        // parser.object_table = vec![
        //     Archivable::Object,
        //     Archivable::Class(Class {
        //         name: "NSMutableAttributedString".to_string(),
        //         version: 0,
        //     }),
        //     Archivable::Class(Class {
        //         name: "NSAttributedString".to_string(),
        //         version: 0,
        //     }),
        //     Archivable::Class(Class {
        //         name: "NSObject".to_string(),
        //         version: 0,
        //     }),
        //     Archivable::Object, // body text
        //     Archivable::Class(Class {
        //         name: "NSMutableString".to_string(),
        //         version: 1,
        //     }),
        //     Archivable::Class(Class {
        //         name: "NSString".to_string(),
        //         version: 1,
        //     }),
        //     Archivable::Object, // key name
        //     Archivable::Class(Class {
        //         name: "NSDictionary".to_string(),
        //         version: 0,
        //     }),
        //     Archivable::Object, // value data
        //     Archivable::Object, // unknown
        //     Archivable::Class(Class {
        //         name: "NSNumber".to_string(),
        //         version: 0,
        //     }),
        //     Archivable::Class(Class {
        //         name: "NSValue".to_string(),
        //         version: 0,
        //     }),
        // ];
        println!("{parser:?}");
        let result = parser.parse();

        println!("\n\nGot data!");
        result.iter().for_each(|item| println!("\n{item:?}"))

        // let expected = "Noter test".to_string();

        // assert_eq!(parsed, expected);
    }

    #[test]
    fn test_parse_text_multi_part() {
        let plist_path = current_dir()
            .unwrap()
            .as_path()
            .join("test_data/streamtyped/Multipart");
        let mut file = File::open(plist_path).unwrap();
        let mut bytes = vec![];
        file.read_to_end(&mut bytes).unwrap();

        let mut parser = TypedStreamReader::new(&bytes);
        println!("{parser:?}");
        let result = parser.parse();

        println!("\n\nGot data!");
        result.iter().for_each(|item| println!("\n{item:?}"))

        // let expected = "Noter test".to_string();

        // assert_eq!(parsed, expected);
    }
}
