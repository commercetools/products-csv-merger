extern crate csv;
extern crate difference;
extern crate term;

use csv::StringRecord;
use difference::{Difference, Changeset};
use std::collections::HashMap;
use std::collections::HashSet;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::process;

type Record = HashMap<String, String>;

fn to_record(headers: &StringRecord, row: &StringRecord) -> Record {
    headers
        .iter()
        .zip(row.iter())
        .map(|(h, r)| (String::from(h), String::from(r)))
        .collect()
}

fn display_diff(text1: &str, text2: &str) -> io::Result<()> {
    let Changeset { diffs, .. } = Changeset::new(text1, text2, "\n");

    let mut t = term::stdout().unwrap();

    for i in 0..diffs.len() {
        match diffs[i] {
            Difference::Same(ref x) => {
                t.reset()?;
                writeln!(t, " {}", x)?;
            }
            Difference::Add(ref x) => {
                match diffs[i - 1] {
                    Difference::Rem(ref y) => {
                        t.fg(term::color::GREEN)?;
                        write!(t, "+")?;
                        let Changeset { diffs, .. } = Changeset::new(y, x, " ");
                        for c in diffs {
                            match c {
                                Difference::Same(ref z) => {
                                    t.fg(term::color::GREEN)?;
                                    write!(t, "{}", z)?;
                                    write!(t, " ")?;
                                }
                                Difference::Add(ref z) => {
                                    t.fg(term::color::WHITE)?;
                                    t.bg(term::color::GREEN)?;
                                    write!(t, "{}", z)?;
                                    t.reset()?;
                                    write!(t, " ")?;
                                }
                                _ => (),
                            }
                        }
                        writeln!(t, "")?;
                    }
                    _ => {
                        t.fg(term::color::BRIGHT_GREEN)?;
                        writeln!(t, "+{}", x)?;
                    }
                };
            }
            Difference::Rem(ref x) => {
                t.fg(term::color::RED)?;
                writeln!(t, "-{}", x)?;
            }
        }
    }
    t.reset()?;
    t.flush()
}


// key to compare if:
// - name.de (to be discussed as only set on product)
// - custom attributes (CamelCase) except the ones set on master
fn should_compare_key(key: &str) -> bool {
    //    key == "name.de" ||
    (key.chars().next().iter().any(|c| c.is_uppercase()) && key != "ConsiderForSearch" &&
         key != "ContentDescription" &&
         key != "PartnerProduct" && key != "PartnerShop" && key != "PartnerShops" &&
         key != "QAValidation" && key != "QAValidationMessage" &&
         key != "RedaktionellerContent" &&
         key != "Validation" &&
         key != "ValidationMessage" && key != "ValidationException")
}

fn is_master(r: &Record) -> bool {
    !r.get("_published").iter().all(|p| p.is_empty())
}

fn run<R, W>(
    mut master_rdr: csv::Reader<R>,
    mut partner_rdr: csv::Reader<R>,
    wtr: &mut csv::Writer<W>,
    accept_all_changes: bool,
) -> Result<(), Box<Error>>
where
    R: std::io::Read,
    W: std::io::Write,
{

    // put all partners records into memory (HashMap sku -> field key -> field value)
    let partner_headers = partner_rdr.headers()?.clone();
    let partner_records: HashMap<String, Record> = partner_rdr
        .into_records()
        .map(|r| {
            let r = r.unwrap();
            let mut record = to_record(&partner_headers, &r);

            // copy the 'msku' field into 'sku' as it is the master sku, used to identify product
            let sku = record.get("msku").expect("msku column not found").clone();
            record.insert(String::from("sku"), sku.clone());

            let name_exists = record.get("name.de").is_some();
            if name_exists {
                let name = record.get("name.de").unwrap().clone();
                record.insert(String::from("PartnerDescription.de"), name);
            }

            (sku, record)
        })
        .collect();

    // start reading master data
    let master_headers = master_rdr.headers().expect("no data in master").clone();

    let m: HashSet<_> = master_headers.iter().map(String::from).collect();
    let p: HashSet<_> = partner_headers.iter().map(String::from).collect();
    println!();
    println!("structural differences:");
    display_diff(
        &format!("master: {:?}", m.difference(&p)),
        &format!("partner: {:?}", p.difference(&m)),
    )?;
    println!();

    wtr.write_record(&master_headers)?;

    let mut all_records = master_rdr.into_records();
    let unknown = String::from("<unknown>");
    let empty_string = String::from("");

    // first one is a master variant
    if let Some(master_variant) = all_records.next() {
        let master_variant = master_variant?;

        // keep track of the last master_record
        let mut master_record: Record = to_record(&master_headers, &master_variant);
        wtr.write_record(&master_variant)?;

        while let Some(variant) = all_records.next() {
            let variant = variant?;
            let variant_record = to_record(&master_headers, &variant);

            if is_master(&variant_record) {
                // master variant
                master_record = variant_record;
                wtr.write_record(&variant)?;
            } else {
                // variant
                let mut modify_variant = false;
                if let Some(sku) = variant_record.get("sku") {
                    if let Some(partner) = partner_records.get(sku) {
                        modify_variant = true;
                        let mut modified_variant = StringRecord::new();
                        for key in master_headers.iter() {
                            let master_value = variant_record.get(key).unwrap_or(&empty_string);
                            let mut modified_value_written = false;
                            if should_compare_key(key) {
                                let partner_value = partner.get(key).unwrap_or(&empty_string);
                                if master_value != partner_value {
                                    let master_record = master_record.clone();
                                    let name = master_record.get("name.de").unwrap_or(&unknown);
                                    println!(
                                        "# Key '{}' on product '{}' with name '{}'",
                                        key,
                                        sku,
                                        name
                                    );
                                    // println!("Master project  : {}", &master_value);
                                    // println!("Partner project : {}", &partner_value);
                                    display_diff(master_value, partner_value)?;
                                    println!();

                                    if accept_all_changes {
                                        modified_variant.push_field(partner_value);
                                        modified_value_written = true
                                    }
                                }
                            }
                            if !modified_value_written {
                                modified_variant.push_field(master_value);
                            }
                        }
                        wtr.write_record(&modified_variant)?;
                    }
                }

                if !modify_variant {
                    wtr.write_record(&variant)?;
                }
            }
        }
    }

    wtr.flush()?;
    Ok(())
}

/// Returns the first positional argument sent to this process. If there are no
/// positional arguments, then this returns an error.
fn get_arg(n: usize) -> Result<OsString, Box<Error>> {
    match env::args_os().nth(n) {
        None => Err(From::from(
            format!("expected {} argument(s), but got none", n),
        )),
        Some(file_path) => Ok(file_path),
    }
}


fn main() {
    let result_file_path = get_arg(3).unwrap();
    let partner_file_path = get_arg(2).unwrap();
    let master_file_path = get_arg(1).unwrap();

    let master_file = File::open(master_file_path).unwrap();
    let master_rdr = csv::ReaderBuilder::new().flexible(true).from_reader(
        master_file,
    );

    let partner_file = File::open(partner_file_path).unwrap();
    let partner_rdr = csv::ReaderBuilder::new().flexible(true).from_reader(
        partner_file,
    );

    let mut wtr = csv::WriterBuilder::new()
        .flexible(true)
        .from_path(result_file_path)
        .unwrap();


    if let Err(err) = run(master_rdr, partner_rdr, &mut wtr, true) {
        println!("{}", err);
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use csv::{Reader, Writer};

    macro_rules! map(
        { $($key:expr => $value:expr),+ } => {
            {
                let mut m = ::std::collections::HashMap::new();
                $(
                    m.insert(String::from($key), String::from($value));
                )+
                m
            }
         };
    );

    #[test]
    fn test_should_compare_key() {
        assert_eq!(should_compare_key("bla"), false);
        assert_eq!(should_compare_key("Bla"), true);
        assert_eq!(should_compare_key("ContentDescription"), false);
        assert_eq!(should_compare_key(""), false);
    }

    #[test]
    fn test_is_master() {
        assert_eq!(is_master(&(map!{ "_published" => "true" })), true);
        assert_eq!(is_master(&(map!{ "_published" => "false" })), true);
        assert_eq!(is_master(&(map!{ "_published" => "" })), false);
        assert_eq!(is_master(&(map!{ "hello" => "" })), false);
    }

    fn test_run(master_data: &str, partner_data: &str, expected: &str) {
        let master = Reader::from_reader(master_data.as_bytes());
        let partner = Reader::from_reader(partner_data.as_bytes());
        let mut result = Writer::from_writer(vec![]);
        run(master, partner, &mut result, true).unwrap();
        let data = String::from_utf8(result.into_inner().unwrap()).unwrap();
        assert_eq!(data, expected);
    }

    #[test]
    fn master_data_is_copied() {
        let master_data = "\
_published,sku,Att1
true,1,hello
,2,bye
false,3,name
,4,name
";
        test_run(master_data, "", master_data);
    }

    #[test]
    fn do_nothing_if_no_master_data() {
        let master_data = "_published,sku,Att1\n";
        test_run(master_data, "", master_data);
    }

    #[test]
    fn master_data_is_changed_and_copied() {
        let master_data = "\
_published,sku,Att1
true,1,hello
,2,bye
false,3,name
,4,name
";
        let partner_data = "\
msku,Att1
2,bye2
";
        let expected_data = "\
_published,sku,Att1
true,1,hello
,2,bye2
false,3,name
,4,name
";
        test_run(master_data, partner_data, expected_data);
    }

    #[test]
    fn handle_missing_column() {
        let master_data = "\
_published,sku,Att1,Att2
true,1,hello,abc
,2,bye,def
false,3,name,ghi
,4,name,klm
";
        let partner_data = "\
msku,Att1
2,bye2
";
        // a missing column is shown as a diff, and therefor remove the master value
        let expected_data = "\
_published,sku,Att1,Att2
true,1,hello,abc
,2,bye2,
false,3,name,ghi
,4,name,klm
";
        test_run(master_data, partner_data, expected_data);
    }
}
