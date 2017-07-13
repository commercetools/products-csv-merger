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
    (key.chars().next().unwrap().is_uppercase() && key != "ConsiderForSearch" &&
         key != "ContentDescription" && key != "PartnerProduct" && key != "PartnerShop" &&
         key != "PartnerShops" &&
         key != "QAValidation" && key != "QAValidationMessage" &&
         key != "RedaktionellerContent" &&
         key != "Validation" &&
         key != "ValidationMessage" && key != "ValidationException")
}

fn run() -> Result<(), Box<Error>> {
    let result_file_path = get_arg(3)?;
    let partner_file_path = get_arg(2)?;
    let master_file_path = get_arg(1)?;

    let partner_file = File::open(partner_file_path)?;
    let mut partner_rdr = csv::ReaderBuilder::new().flexible(true).from_reader(
        partner_file,
    );

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

    //    println!("{:?}", partner_records.iter().next());
    let master_file = File::open(master_file_path)?;
    let mut master_rdr = csv::ReaderBuilder::new().flexible(true).from_reader(
        master_file,
    );

    let master_headers = master_rdr.headers()?.clone();

    let mut wtr = csv::WriterBuilder::new().flexible(true).from_path(
        result_file_path,
    )?;
    wtr.write_record(&partner_headers)?;

    let m: HashSet<_> = master_headers.iter().map(String::from).collect();
    let p: HashSet<_> = partner_headers.iter().map(String::from).collect();
    println!();
    println!("structural differences:");
    display_diff(
        &format!("master: {:?}", m.difference(&p)),
        &format!("partner: {:?}", p.difference(&m)),
    )?;
    println!();

    let mut all_records = master_rdr.into_records();
    let unknown = String::from("<unknown>");
    let empty_string = String::from("");

    // first one is a master variant
    if let Some(master_variant) = all_records.next() {
        let master_variant = master_variant?;

        // keep track of the last master_record
        let mut master_record: Record = to_record(&master_headers, &master_variant);

        while let Some(variant) = all_records.next() {
            let variant = variant?;
            let variant_record = to_record(&master_headers, &variant);

            if !variant_record.get("_published").iter().all(
                |p| p.is_empty(),
            )
            {
                // master variant
                master_record = variant_record;
                wtr.write_record(&variant)?;
            } else {
                // variant
                if let Some(sku) = variant_record.get("sku") {
                    if let Some(partner) = partner_records.get(sku) {
                        for key in variant_record.keys() {
                            if should_compare_key(key) {
                                let master_value = variant_record.get(key).unwrap_or(&empty_string);
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
                                }
                            }
                        }
                    }
                }

                // TODO: write modified variant?
                wtr.write_record(&variant)?;
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
    if let Err(err) = run() {
        println!("{}", err);
        process::exit(1);
    }
}
