extern crate clap;
extern crate csv;
extern crate difference;
extern crate term;

use csv::StringRecord;
use difference::{Difference, Changeset};
use std::collections::HashMap;
use std::collections::HashSet;
use std::error::Error;
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

fn is_master_variant(r: &Record) -> bool {
    !r.get("_published").iter().all(|p| p.is_empty())
}

fn handle_diff<'a>(
    master_value: &'a str,
    partner_value: &'a str,
    accept_all_changes: bool,
) -> String {
    // println!("Master project  : {}", &master_value);
    // println!("Partner project : {}", &partner_value);
    display_diff(master_value, partner_value).unwrap();

    let result = if accept_all_changes {
        String::from(partner_value)
    } else {
        loop {
            println!("(p)artner (default) or (m)aster or (e)dit?");
            let mut input = String::new();
            io::stdin().read_line(&mut input).expect(
                "failed to read line",
            );
            let input = input.replace("\r\n", "");
            let input = input.replace("\n", "");
            match Some(&*input) {
                Some("p") | Some("") => return String::from(partner_value),
                Some("m") => return String::from(master_value),
                Some("e") => {
                    println!("Enter new value:");
                    let mut new_value = String::new();
                    io::stdin().read_line(&mut new_value).expect(
                        "failed to read line",
                    );
                    return new_value;
                }
                _ => continue,
            }
        }
    };
    println!();
    result
}

fn write_record<W>(
    headers: &StringRecord,
    r: &Record,
    wtr: &mut csv::Writer<W>,
) -> Result<(), Box<Error>>
where
    W: std::io::Write,
{

    let mut to_write = StringRecord::new();
    let empty_string = String::from("");

    for h in headers.iter() {
        to_write.push_field(r.get(h).unwrap_or(&empty_string));
    }
    Ok(wtr.write_record(&to_write)?)
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

    let mut master_variant: Option<Record> = None;
    let mut master_variant_to_write: Option<Record> = None;
    while let Some(variant) = all_records.next() {
        let variant = variant?;
        let variant_record = to_record(&master_headers, &variant);
        let mut variant_to_write = variant_record.clone();

        if is_master_variant(&variant_record) {
            if let Some(m) = master_variant_to_write {
                write_record(&master_headers, &m, wtr)?;
            }
            master_variant_to_write = Some(variant_record.clone());
            master_variant = Some(variant_record.clone()).clone();
        } else {
            // variant
            if let Some(sku) = variant_record.get("sku") {
                if let Some(partner) = partner_records.get(sku) {
                    for key in master_headers.iter() {
                        let master_value = variant_record.get(key).unwrap_or(&empty_string);

                        if should_compare_key(key) {
                            if let Some(partner_value) = partner.get(key) {
                                if master_value != partner_value {
                                    let product_name = if let Some(m) = master_variant.clone() {
                                        m.get("name.de").map(|n| n.clone()).unwrap_or(
                                            unknown.clone(),
                                        )
                                    } else {
                                        unknown.clone()
                                    };
                                    println!(
                                        "# Key '{}' on product '{}' with name '{}'",
                                        key,
                                        sku,
                                        product_name
                                    );
                                    let new_value = handle_diff(
                                        master_value,
                                        partner_value,
                                        accept_all_changes,
                                    );
                                    variant_to_write.insert(String::from(key), new_value.clone());
                                    if let Some(mut m) = master_variant_to_write.take() {
                                        m.insert(String::from(key), new_value);
                                        master_variant_to_write = Some(m);
                                    }
                                }
                            }
                        } else if key == "name.de" || key == "description.de" {
                            if let Some(partner_name) = partner.get(key) {
                                if let Some(master_name) =
                                    master_variant.clone().and_then(|m| {
                                        m.get(key).map(|n| n.clone())
                                    })
                                {
                                    if *partner_name != master_name {
                                        println!(
                                            "# Key '{}' on product '{}' changed",
                                            key,
                                            sku
                                        );
                                        let new_value = handle_diff(
                                            &master_name,
                                            partner_name,
                                            accept_all_changes,
                                        );
                                        if let Some(mut m) = master_variant_to_write.take() {
                                            m.insert(String::from(key), new_value);
                                            master_variant_to_write = Some(m);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // write the master variant before the variants if needed
            if let Some(m) = master_variant_to_write {
                write_record(&master_headers, &m, wtr)?;
                master_variant_to_write = None;
            }
            write_record(&master_headers, &variant_to_write, wtr)?;
        }
    }
    if let Some(m) = master_variant_to_write {
        write_record(&master_headers, &m, wtr)?;
    }

    wtr.flush()?;
    Ok(())
}

fn main() {
    let matches = clap::App::new("products-merger")
        .version("1.0")
        .author("Yann Simon <yann.simon@commercetools.com>")
        .args_from_usage(
            "<MASTER_FILE> 'CSV export of master project' \n\
             <PARTNER_FILE> 'CSV export of partner project' \n\
             <RESULT_FILE> 'result CSV file' \n\
            --accept-all=[true|false] 'accepts all changes from partner project (default: false)'",
        )
        .get_matches();

    let master_file_path = matches.value_of("MASTER_FILE").unwrap();
    let partner_file_path = matches.value_of("PARTNER_FILE").unwrap();
    let result_file_path = matches.value_of("RESULT_FILE").unwrap();

    let accept_all = matches.value_of("accept-all").iter().any(|&a| a == "true");
    println!(
        "Merging master export '{}' with partner export '{}' to '{}' (accept-all={})",
        master_file_path,
        partner_file_path,
        result_file_path,
        accept_all
    );

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


    if let Err(err) = run(master_rdr, partner_rdr, &mut wtr, accept_all) {
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
        assert_eq!(is_master_variant(&(map!{ "_published" => "true" })), true);
        assert_eq!(is_master_variant(&(map!{ "_published" => "false" })), true);
        assert_eq!(is_master_variant(&(map!{ "_published" => "" })), false);
        assert_eq!(is_master_variant(&(map!{ "hello" => "" })), false);
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
true,1,bye2
,2,bye2
false,3,name
,4,name
";
        test_run(master_data, partner_data, expected_data);
    }

    #[test]
    fn compare_partner_description() {
        let master_data = "\
_published,sku,PartnerDescription.de
true,1,hello
,2,bye
false,3,name
,4,name
";
        let partner_data = "\
msku,PartnerDescription.de
2,bye2
";
        let expected_data = "\
_published,sku,PartnerDescription.de
true,1,bye2
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
        // a missing column is ignored
        let expected_data = "\
_published,sku,Att1,Att2
true,1,bye2,abc
,2,bye2,def
false,3,name,ghi
,4,name,klm
";
        test_run(master_data, partner_data, expected_data);
    }

    #[test]
    fn handle_product_with_only_master_variant() {
        let master_data = "\
_published,sku,Att1
true,1,hello
false,3,name
,4,name
";
        let partner_data = "\
msku,Att1
4,bye2
";
        let expected_data = "\
_published,sku,Att1
true,1,hello
false,3,bye2
,4,bye2
";
        test_run(master_data, partner_data, expected_data);
    }

    #[test]
    fn update_product_name() {
        let master_data = "\
_published,sku,name.de
true,1,v1
,2,
false,3,v2
,4,
";
        let partner_data = "\
msku,name.de
2,v1b
";
        let expected_data = "\
_published,sku,name.de
true,1,v1b
,2,
false,3,v2
,4,
";
        test_run(master_data, partner_data, expected_data);
    }

    #[test]
    fn update_product_description() {
        let master_data = "\
_published,sku,description.de
true,1,v1
,2,
false,3,v2
,4,
";
        let partner_data = "\
msku,description.de
2,v1b
";
        let expected_data = "\
_published,sku,description.de
true,1,v1b
,2,
false,3,v2
,4,
";
        test_run(master_data, partner_data, expected_data);
    }
}
