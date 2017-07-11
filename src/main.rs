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
use std::process;

type Record = HashMap<String, String>;

fn to_record(headers: &StringRecord, row: &StringRecord) -> Record {
    headers
        .iter()
        .zip(row.iter())
        .map(|(h, r)| (String::from(h), String::from(r)))
        .collect()
}

fn display_diff(text1: &str, text2: &str) {
    let Changeset { diffs, .. } = Changeset::new(text1, text2, "\n");

    let mut t = term::stdout().unwrap();

    for i in 0..diffs.len() {
        match diffs[i] {
            Difference::Same(ref x) => {
                t.reset().unwrap();
                writeln!(t, " {}", x).unwrap();
            }
            Difference::Add(ref x) => {
                match diffs[i - 1] {
                    Difference::Rem(ref y) => {
                        t.fg(term::color::GREEN).unwrap();
                        write!(t, "+").unwrap();
                        let Changeset { diffs, .. } = Changeset::new(y, x, " ");
                        for c in diffs {
                            match c {
                                Difference::Same(ref z) => {
                                    t.fg(term::color::GREEN).unwrap();
                                    write!(t, "{}", z).unwrap();
                                    write!(t, " ").unwrap();
                                }
                                Difference::Add(ref z) => {
                                    t.fg(term::color::WHITE).unwrap();
                                    t.bg(term::color::GREEN).unwrap();
                                    write!(t, "{}", z).unwrap();
                                    t.reset().unwrap();
                                    write!(t, " ").unwrap();
                                }
                                _ => (),
                            }
                        }
                        writeln!(t, "").unwrap();
                    }
                    _ => {
                        t.fg(term::color::BRIGHT_GREEN).unwrap();
                        writeln!(t, "+{}", x).unwrap();
                    }
                };
            }
            Difference::Rem(ref x) => {
                t.fg(term::color::RED).unwrap();
                writeln!(t, "-{}", x).unwrap();
            }
        }
    }
    t.reset().unwrap();
    t.flush().unwrap();
}

fn run() -> Result<(), Box<Error>> {
    let result_file_path = get_arg(3)?;
    let partner_file_path = get_arg(2)?;
    let master_file_path = get_arg(1)?;

    let partner_file = File::open(partner_file_path)?;
    let mut partner_rdr = csv::ReaderBuilder::new().flexible(true).from_reader(
        partner_file,
    );

    let partner_headers = partner_rdr.headers()?.clone();
    let partner_records: HashMap<String, Record> = partner_rdr
        .into_records()
        .map(|r| {
            let r = r.unwrap();
            let mut record = to_record(&partner_headers, &r);

            // copy the 'msku' field into 'sku' as it is the master sku, used to identify product
            let sku = record.get("msku").unwrap().clone();
            record.insert(String::from("sku"), sku.clone());
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
    );
    println!();

    let mut all_records = master_rdr.into_records().take(2);
    let unknown = String::from("<unknown>");
    let absent = String::from("<absent>");

    while let Some(master_variant) = all_records.next() {
        let master_variant = master_variant.unwrap();
        let master_record = to_record(&master_headers, &master_variant);
        wtr.write_record(&master_variant).unwrap();

        while let Some(variant) = all_records.next() {
            let variant = variant.unwrap();
            let variant_record = to_record(&master_headers, &variant);

            if let Some(sku) = variant_record.get("sku") {
                if let Some(partner) = partner_records.get(sku) {
                    for key in master_record.keys() {
                        if
                        //key.chars().next().unwrap().is_uppercase() &&
                        variant_record.get(key) != partner.get(key) {
                            println!(
                                "Key '{}' on product '{}' with name '{}'",
                                key,
                                sku,
                                master_record.get("name.de").unwrap_or(&unknown)
                            );
                            println!(
                                "Master project - master variant : {}",
                                &master_record.get(key).unwrap_or(&absent)
                            );
                            println!(
                                "Master project - first variant  : {}",
                                &variant_record.get(key).unwrap_or(&absent)
                            );
                            println!(
                                "Partner project                 : {}",
                                &partner.get(key).unwrap_or(&absent)
                            );
                            if let Some(m) = master_record.get(key) {
                                if let Some(v) = partner.get(key) {
                                    display_diff(m, v);
                                }
                            }
                            println!();
                        }
                    }
                }
            }

            // TODO: write modified variant?
            wtr.write_record(&variant).unwrap();
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
