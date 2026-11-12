use super::*;

fn table_with(items: &[(&str, &str)]) -> MacroTable {
    let mut table = MacroTable::new();
    for (key, text) in items {
        table.insert(key, text).unwrap();
    }
    table
}

#[test]
fn expands_with_the_case_of_the_typed_key() {
    let table = table_with(&[("vn", "Việt Nam")]);
    assert_eq!(table.expand("vn").unwrap(), "việt nam");
    assert_eq!(table.expand("VN").unwrap(), "VIỆT NAM");
    assert_eq!(table.expand("Vn").unwrap(), "Việt Nam");
}

#[test]
fn single_capital_letter_key_gives_capitals() {
    let table = table_with(&[("k", "không")]);
    assert_eq!(table.expand("K").unwrap(), "KHÔNG");
}

#[test]
fn key_starting_with_a_symbol_leaves_text_unchanged() {
    let table = table_with(&[("@vn", "Việt Nam")]);
    assert_eq!(table.expand("@vn").unwrap(), "Việt Nam");
}

#[test]
fn vietnamese_letters_fold_case_and_normalization() {
    let table = table_with(&[("đc", "được")]);
    assert_eq!(table.expand("ĐC").unwrap(), "ĐƯỢC");
    let decomposed_key = table_with(&[("ho\u{0301}a", "hóa học")]);
    assert_eq!(decomposed_key.lookup("hóa"), Some("hóa học"));
}

#[test]
fn lookup_misses_unknown_keys() {
    let table = table_with(&[("vn", "Việt Nam")]);
    assert_eq!(table.lookup("hcm"), None);
    assert_eq!(table.expand("hcm"), None);
}

#[test]
fn later_insert_replaces_earlier_one_case_insensitively() {
    let table = table_with(&[("vn", "Việt Nam"), ("VN", "Vietnam")]);
    assert_eq!(table.len(), 1);
    assert_eq!(table.lookup("vn"), Some("Vietnam"));
}

#[test]
fn key_is_cut_to_fifteen_characters() {
    let table = table_with(&[("abcdefghijklmnopqrst", "x")]);
    assert_eq!(table.lookup("abcdefghijklmno"), Some("x"));
    assert_eq!(table.items()[0].0, "abcdefghijklmno");
}

#[test]
fn rejects_empty_keys_and_a_full_table() {
    let mut table = MacroTable::new();
    assert_eq!(table.insert("", "x"), Err(MacroError::EmptyKey));
    for number in 0..MAX_ITEMS {
        table.insert(&format!("k{number}"), "x").unwrap();
    }
    assert_eq!(table.insert("overflow", "x"), Err(MacroError::TableFull));
    assert_eq!(table.insert("k0", "y"), Ok(()));
}

#[test]
fn remove_deletes_by_folded_key() {
    let mut table = table_with(&[("vn", "Việt Nam")]);
    assert!(table.remove("VN"));
    assert!(!table.remove("vn"));
    assert!(table.is_empty());
}

#[test]
fn parses_version_one_with_bom_header_crlf_and_colons_in_text() {
    let file = "\u{FEFF};DO NOT DELETE THIS LINE*** version=1 ***\r\nvn:Việt Nam\r\ntime:12:30\r\nnocolon\r\n:empty key\r\n";
    let loaded = MacroTable::parse(file.as_bytes());
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.table.len(), 2);
    assert_eq!(loaded.table.lookup("vn"), Some("Việt Nam"));
    assert_eq!(loaded.table.lookup("time"), Some("12:30"));
}

#[test]
fn parses_the_portable_header_without_bom_or_semicolon() {
    let loaded =
        MacroTable::parse(b"DO NOT DELETE THIS LINE*** version=1 ***\nvn:Vi\xE1\xBB\x87t Nam");
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.table.lookup("vn"), Some("Việt Nam"));
}

#[test]
fn headerless_file_is_version_zero_viqr() {
    let loaded = MacroTable::parse(b"vn:Vie^.t Nam\r\nddc:ddu+o+.c\r\n");
    assert_eq!(loaded.version, 0);
    assert_eq!(loaded.table.lookup("vn"), Some("Việt Nam"));
    assert_eq!(loaded.table.lookup("đc"), Some("được"));
}

#[test]
fn serialize_writes_header_sorted_lines_and_no_trailing_newline() {
    let table = table_with(&[("vn", "Việt Nam"), ("hcm", "Hồ Chí Minh")]);
    let contents = table.serialize();
    assert_eq!(
        contents,
        "\u{FEFF};DO NOT DELETE THIS LINE*** version=1 ***\nhcm:Hồ Chí Minh\nvn:Việt Nam"
    );
}

#[test]
fn serialize_then_parse_round_trips() {
    let table = table_with(&[
        ("vn", "Việt Nam"),
        ("Đc", "được: ok"),
        ("tp.hcm", "TP. Hồ Chí Minh"),
    ]);
    let loaded = MacroTable::parse(table.serialize().as_bytes());
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.table.items(), table.items());
}

#[test]
fn saves_and_loads_a_file() {
    let path = std::env::temp_dir().join("oh-key-macro-test-macros.txt");
    let table = table_with(&[("vn", "Việt Nam")]);
    table.save(&path).unwrap();
    let loaded = MacroTable::load(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(loaded.table.lookup("VN"), Some("Việt Nam"));
}
