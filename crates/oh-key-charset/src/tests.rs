use super::*;
use crate::standard::{FIRST_SYMBOL_INDEX, char_from_glyph};

/// Every Vietnamese letter, in standard order.
fn vietnamese_letters() -> Vec<char> {
    (0..FIRST_SYMBOL_INDEX)
        .map(|index| char_from_glyph(Glyph::Standard(index)))
        .collect()
}

/// The bytes a legacy charset assigns to each standard character, if it is
/// table driven. `None` for the Unicode family and VIQR.
fn code_table(charset: Charset) -> Option<Vec<Vec<u8>>> {
    let words = |table: &[u16]| -> Vec<Vec<u8>> {
        table
            .iter()
            .map(|&word| {
                let [low, high] = word.to_le_bytes();
                if high == 0 {
                    vec![low]
                } else {
                    vec![low, high]
                }
            })
            .collect()
    };
    match charset {
        Charset::Tcvn3 => Some(
            t::SINGLE_BYTE_TCVN3
                .iter()
                .map(|&byte| vec![byte])
                .collect(),
        ),
        Charset::Vps => Some(t::SINGLE_BYTE_VPS.iter().map(|&byte| vec![byte]).collect()),
        Charset::Viscii => Some(
            t::SINGLE_BYTE_VISCII
                .iter()
                .map(|&byte| vec![byte])
                .collect(),
        ),
        Charset::BkHcm1 => Some(
            t::SINGLE_BYTE_BK_HCM_1
                .iter()
                .map(|&byte| vec![byte])
                .collect(),
        ),
        Charset::VietwareF => Some(
            t::SINGLE_BYTE_VIETWARE_F
                .iter()
                .map(|&byte| vec![byte])
                .collect(),
        ),
        Charset::Isc => Some(t::SINGLE_BYTE_ISC.iter().map(|&byte| vec![byte]).collect()),
        Charset::VniWindows => Some(words(&t::DOUBLE_BYTE_VNI_WINDOWS)),
        Charset::BkHcm2 => Some(words(&t::DOUBLE_BYTE_BK_HCM_2)),
        Charset::VietwareX => Some(words(&t::DOUBLE_BYTE_VIETWARE_X)),
        Charset::VniMac => Some(words(&t::DOUBLE_BYTE_VNI_MAC)),
        Charset::WindowsCp1258 => Some(words(&t::WIN_CP1258_COMPOSITE)),
        _ => None,
    }
}

#[test]
fn tables_cover_all_213_standard_characters() {
    assert_eq!(t::STANDARD_CHARACTER_COUNT, 213);
    assert_eq!(vietnamese_letters().len(), 186);
}

#[test]
fn every_vietnamese_letter_round_trips_in_every_charset() {
    for charset in Charset::ALL {
        let table = code_table(charset);
        for (index, letter) in vietnamese_letters().into_iter().enumerate() {
            let text = letter.to_string();
            let encoded = encode(&text, charset);
            if let Some(table) = &table {
                assert_eq!(encoded, table[index], "{charset:?} {letter}");
            }
            let decoded = decode(&encoded, charset);
            let shares_code = table.as_ref().is_some_and(|table| {
                table
                    .iter()
                    .enumerate()
                    .any(|(other, code)| other != index && *code == table[index])
            });
            if shares_code {
                // TCVN3 style charsets write capital toned letters as lower case.
                assert_eq!(encode(&decoded, charset), encoded, "{charset:?} {letter}");
            } else {
                assert_eq!(decoded, text, "{charset:?} {letter}");
            }
        }
    }
}

#[test]
fn letters_separated_by_spaces_round_trip_in_one_pass() {
    let text = vietnamese_letters()
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>()
        .join(" ");
    for charset in Charset::ALL {
        let decoded = decode(&encode(&text, charset), charset);
        let folds_capitals = matches!(
            charset,
            Charset::Tcvn3
                | Charset::Vps
                | Charset::Viscii
                | Charset::BkHcm1
                | Charset::VietwareF
                | Charset::Isc
        );
        if !folds_capitals {
            assert_eq!(decoded, text, "{charset:?}");
        }
    }
}

#[test]
fn mixed_sentence_round_trips_in_every_charset() {
    let text = "Tiếng Việt: ưu tiên, đường phố? 100% (ok) end.";
    for charset in Charset::ALL {
        let decoded = decode(&encode(text, charset), charset);
        assert_eq!(decoded, text, "{charset:?}");
    }
}

#[test]
fn viqr_marks_typed_as_plain_text_round_trip() {
    let text = "a'b c(d e+f g^h x~y z.w \\ end.";
    for charset in Charset::ALL {
        // BK HCM 1 and ISC use ^ ~ ` | { } or \ for letters, so they cannot hold these.
        if matches!(charset, Charset::BkHcm1 | Charset::Isc) {
            continue;
        }
        let decoded = decode(&encode(text, charset), charset);
        assert_eq!(decoded, text, "{charset:?}");
    }
}

#[test]
fn known_encodings() {
    let text = "Việt Nam";
    assert_eq!(encode(text, Charset::Tcvn3), b"Vi\xD6t Nam");
    assert_eq!(encode(text, Charset::VniWindows), b"Vie\xE4t Nam");
    assert_eq!(encode(text, Charset::Viqr), b"Vie^.t Nam");
    assert_eq!(encode(text, Charset::Utf8), text.as_bytes());
    assert_eq!(encode(text, Charset::NcrDecimal), b"Vi&#7879;t Nam");
    assert_eq!(encode(text, Charset::NcrHexadecimal), b"Vi&#x1EC7;t Nam");
    assert_eq!(encode(text, Charset::CString), b"Vi\\x1EC7t N\\x61m");
    assert_eq!(
        encode("ệ", Charset::UnicodeDecomposed),
        [0xEA, 0x00, 0x23, 0x03]
    );
    assert_eq!(encode("ệ", Charset::Unicode), [0xC7, 0x1E]);
}

#[test]
fn unicode_family_keeps_symbols_legacy_charsets_pad_them() {
    let text = "“quote” € …";
    for charset in [
        Charset::Unicode,
        Charset::Utf8,
        Charset::NcrDecimal,
        Charset::NcrHexadecimal,
    ] {
        assert_eq!(decode(&encode(text, charset), charset), text, "{charset:?}");
    }
    assert_eq!(encode("“a” …", Charset::Vps), b"\"a\" .");
    assert_eq!(encode("€", Charset::Vps), b"#");
    assert_eq!(
        decode(&encode("“a” …", Charset::Tcvn3), Charset::Tcvn3),
        "“a” …"
    );
}

#[test]
fn bytes_a_charset_does_not_assign_pass_through() {
    assert_eq!(decode(b"\xE9", Charset::Utf8Viqr), "\u{FFFD}");
    assert_eq!(decode(b"~\xFF", Charset::Tcvn3).chars().count(), 2);
    // 0x3F is `?` in VNI-Mac; it must stay a question mark.
    assert_eq!(decode(b"?", Charset::VniMac), "?");
}

#[test]
fn uppercase_toned_tcvn3_letters_decode_as_lower_case() {
    assert_eq!(decode(&encode("Á", Charset::Tcvn3), Charset::Tcvn3), "á");
}

#[test]
fn decomposed_input_is_composed_before_encoding() {
    assert_eq!(encode("e\u{0309}", Charset::Viqr), b"e?");
    assert_eq!(normalize_to_precomposed("ho\u{0301}a"), "hóa");
    assert_eq!(decode(&[0x61, 0x00, 0x01, 0x03], Charset::Unicode), "á");
}

#[test]
fn viqr_escapes_marks_that_would_be_read_as_tones() {
    assert_eq!(encode("a'", Charset::Viqr), b"a\\'");
    assert_eq!(encode("á'", Charset::Viqr), b"a''");
    assert_eq!(encode("â?", Charset::Viqr), b"a^\\?");
    assert_eq!(encode("x'", Charset::Viqr), b"x'");
    assert_eq!(decode(b"a\\'", Charset::Viqr), "a'");
    assert_eq!(decode(b"DDo`i", Charset::Viqr), "Đòi");
}

#[test]
fn viqr_does_not_escape_inside_urls_and_mail_addresses() {
    let text = "xem www.vietnam.vn/a?b hay mail@ao.com";
    assert_eq!(encode(text, Charset::Viqr), text.as_bytes());
    assert_eq!(decode(text.as_bytes(), Charset::Viqr), text);
}

#[test]
fn viqr_reads_dd_only_at_the_start_of_a_word() {
    assert_eq!(decode(b"dduong", Charset::Viqr), "đuong");
    assert_eq!(decode(b"adda", Charset::Viqr), "adda");
}

#[test]
fn utf8_viqr_reads_both_kinds_of_text() {
    assert_eq!(
        decode("Việt Vie^.t".as_bytes(), Charset::Utf8Viqr),
        "Việt Việt"
    );
    assert_eq!(decode("ệ.".as_bytes(), Charset::Utf8Viqr), "ệ.");
    assert_eq!(encode("Việt", Charset::Utf8Viqr), b"Vie^.t");
}

#[test]
fn references_without_a_semicolon_stay_literal() {
    assert_eq!(decode(b"&#7879", Charset::NcrDecimal), "&#7879");
    assert_eq!(
        decode(b"&#x1EC7;&#xZZ;", Charset::NcrHexadecimal),
        "ệ&#xZZ;"
    );
    assert_eq!(decode(b"a & b &#;", Charset::NcrDecimal), "a & b &#;");
}

#[test]
fn convert_goes_between_legacy_charsets() {
    let tcvn3 = encode("Việt Nam đẹp", Charset::Tcvn3);
    let vni = convert(&tcvn3, Charset::Tcvn3, Charset::VniWindows);
    assert_eq!(vni, encode("Việt Nam đẹp", Charset::VniWindows));
    assert_eq!(decode(&vni, Charset::VniWindows), "Việt Nam đẹp");
}

#[test]
fn deletions_count_characters_or_bytes_by_charset() {
    assert_eq!(Charset::Unicode.deletions_for("Việt"), 4);
    assert_eq!(Charset::XUtf8.deletions_for("Việt"), 4);
    assert_eq!(Charset::Utf8.deletions_for("Việt"), 6);
    assert_eq!(Charset::Tcvn3.deletions_for("Việt"), 4);
    assert_eq!(Charset::VniWindows.deletions_for("Việt"), 5);
    assert_eq!(Charset::UnicodeDecomposed.deletions_for("Việt"), 5);
}

#[test]
fn names_resolve_in_any_case() {
    for charset in Charset::ALL {
        assert_eq!(Charset::from_name(charset.name()), Some(charset));
        assert_eq!(
            Charset::from_name(&charset.name().to_lowercase()),
            Some(charset)
        );
    }
    assert_eq!(Charset::from_name("utf8"), Some(Charset::Utf8));
    assert_eq!(Charset::from_name("nope"), None);
}
