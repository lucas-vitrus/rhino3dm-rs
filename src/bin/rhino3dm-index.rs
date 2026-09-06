use rhino3dm_rs::File3dm;
use std::collections::BTreeMap;

fn main() {
    let Some(path) = std::env::args_os().nth(1) else {
        eprintln!("Usage: rhino3dm-index <source.3dm>");
        std::process::exit(2);
    };
    match File3dm::read(&path) {
        Ok(file) => {
            let mut records_by_table = BTreeMap::new();
            for table in &file.archive().tables {
                records_by_table.insert(format!("{:#010x}", table.typecode), table.records.len());
            }
            println!("archive_version={}", file.archive_version());
            println!("tables={}", file.archive().tables.len());
            println!(
                "instance_definitions_decoded={}",
                file.archive().instance_definitions.len()
            );
            println!(
                "instance_definition_members={}",
                file.archive()
                    .instance_definitions
                    .iter()
                    .map(|definition| definition.members.len())
                    .sum::<usize>()
            );
            println!("object_records={}", file.archive().object_count());
            println!(
                "framed_object_records={}",
                file.archive()
                    .objects
                    .iter()
                    .filter(|object| object.framing_error.is_none())
                    .count()
            );
            println!(
                "object_record_framing_errors={}",
                file.archive()
                    .objects
                    .iter()
                    .filter(|object| object.framing_error.is_some())
                    .count()
            );
            println!(
                "attribute_records_parsed={}",
                file.archive()
                    .objects
                    .iter()
                    .filter(|object| object.attributes.is_some())
                    .count()
            );
            println!(
                "attribute_records_complete={}",
                file.archive()
                    .objects
                    .iter()
                    .filter_map(|object| object.attributes.as_ref())
                    .filter(|attributes| attributes.complete)
                    .count()
            );
            println!(
                "attribute_parse_errors={}",
                file.archive()
                    .objects
                    .iter()
                    .filter(|object| object.attribute_error.is_some())
                    .count()
            );
            println!(
                "user_string_records={}",
                file.archive()
                    .objects
                    .iter()
                    .filter_map(|object| object.attributes.as_ref())
                    .filter(|attributes| !attributes.user_strings.is_empty())
                    .count()
            );
            println!(
                "user_strings={}",
                file.archive()
                    .objects
                    .iter()
                    .filter_map(|object| object.attributes.as_ref())
                    .map(|attributes| attributes.user_strings.len())
                    .sum::<usize>()
            );
            println!(
                "attribute_userdata_errors={}",
                file.archive()
                    .objects
                    .iter()
                    .filter(|object| object.attribute_userdata_error.is_some())
                    .count()
            );
            for object in file
                .archive()
                .objects
                .iter()
                .filter(|object| object.framing_error.is_some())
                .take(5)
            {
                println!(
                    "framing_error_at_{}={}",
                    object.source.offset,
                    object.framing_error.as_deref().unwrap_or_default()
                );
            }
            for (table, count) in records_by_table {
                println!("table_{table}_records={count}");
            }
            let mut geometry = BTreeMap::new();
            for object in &file.archive().objects {
                *geometry
                    .entry(format!("{:?}", object.geometry_kind))
                    .or_insert(0_usize) += 1;
            }
            for (kind, count) in geometry {
                println!("geometry_{kind}={count}");
            }
            let mut classes = BTreeMap::new();
            for object in &file.archive().objects {
                if let Some(class_id) = object.class_id {
                    *classes.entry(hex(&class_id)).or_insert(0_usize) += 1;
                }
            }
            let mut classes: Vec<_> = classes.into_iter().collect();
            classes.sort_by_key(|entry| std::cmp::Reverse(entry.1));
            for (class_id, count) in classes.into_iter().take(12) {
                println!("class_{class_id}={count}");
            }
            println!(
                "points_decoded={}",
                file.archive()
                    .objects
                    .iter()
                    .filter(|object| object.point.is_some())
                    .count()
            );
            println!(
                "instance_references_decoded={}",
                file.archive()
                    .objects
                    .iter()
                    .filter(|object| object.instance_reference.is_some())
                    .count()
            );
            println!(
                "geometry_parse_errors={}",
                file.archive()
                    .objects
                    .iter()
                    .filter(|object| object.geometry_error.is_some())
                    .count()
            );
            let mut geometry_errors = BTreeMap::new();
            for object in file
                .archive()
                .objects
                .iter()
                .filter_map(|object| object.geometry_error.as_deref())
            {
                *geometry_errors.entry(object).or_insert(0_usize) += 1;
            }
            for (error, count) in geometry_errors {
                println!("geometry_error_{error}={count}");
            }
        }
        Err(error) => {
            eprintln!("rhino3dm-index: {error}");
            std::process::exit(1);
        }
    }
}

fn hex(value: &[u8; 16]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}
