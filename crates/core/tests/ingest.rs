//! Reading statements: what comes in, what is reported back, and what is never guessed.

// Restated although the workspace already forbids it: an integration test is its own crate
// root, and a test is exactly where someone reaches for a shortcut.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

mod common;

use common::{GHS, USD, id};
use mt_core::enums::Direction;
use mt_core::ids::AccountId;
use mt_core::ingest::csv::{self, AmountColumns, Delimiter, Mapping};
use mt_core::ingest::{
    DateFormat, IngestError, MAX_STATEMENT_BYTES, MAX_STATEMENT_LINES, Problem, Statement, ofx,
    stage,
};
use mt_core::money::Money;
use time::UtcOffset;
use time::macros::{date, datetime, offset};

/// The shape most Ghanaian bank exports take: a header, then date, description, one signed
/// amount and a running balance.
fn signed_mapping() -> Mapping {
    Mapping {
        delimiter: Delimiter::Comma,
        skip_rows: 0,
        has_header: true,
        date: 0,
        date_format: DateFormat::DayMonthYear,
        amount: AmountColumns::Signed { column: 2 },
        label: 1,
        counterparty: None,
        reference: None,
        balance: Some(3),
    }
}

fn read(csv_text: &str) -> Statement {
    csv::read(csv_text.as_bytes(), &signed_mapping(), GHS).unwrap()
}

// --------------------------------------------------------------------------------- csv ----

#[test]
fn a_signed_column_gives_direction_by_sign() {
    let statement = read(
        "Date,Description,Amount,Balance\n\
         03/11/2026,SALARY NOV,\"5,000.00\",\"5,120.00\"\n\
         04/11/2026,POS SHOPRITE,-120.50,\"4,999.50\"\n",
    );

    assert!(statement.problems.is_empty(), "{:?}", statement.problems);
    let [salary, shop] = statement.lines.as_slice() else {
        panic!("expected two lines, got {:?}", statement.lines);
    };
    assert_eq!(salary.direction, Direction::In);
    assert_eq!(salary.amount, Money::new(500_000, GHS));
    assert_eq!(salary.booked_on, date!(2026 - 11 - 03));
    assert_eq!(shop.direction, Direction::Out);
    assert_eq!(shop.amount, Money::new(12_050, GHS));
    assert_eq!(shop.label, "POS SHOPRITE");
    assert_eq!(shop.reported_balance, Some(Money::new(499_950, GHS)));
    assert_eq!(shop.line_no, 3);
}

#[test]
fn split_columns_read_either_side_whatever_sign_the_bank_printed() {
    let mapping = Mapping {
        amount: AmountColumns::Split {
            money_in: 3,
            money_out: 2,
        },
        balance: None,
        ..signed_mapping()
    };
    let text = "Date,Narration,Debit,Credit\n\
                03/11/2026,Transfer in,,200.00\n\
                04/11/2026,ATM,50.00,\n\
                05/11/2026,Fee,-2.00,\n";
    let statement = csv::read(text.as_bytes(), &mapping, GHS).unwrap();

    let moves: Vec<(Direction, i64)> = statement
        .lines
        .iter()
        .map(|line| (line.direction, line.amount.minor()))
        .collect();
    assert_eq!(
        moves,
        vec![
            (Direction::In, 20_000),
            (Direction::Out, 5_000),
            (Direction::Out, 200)
        ]
    );
}

#[test]
fn lines_above_the_header_are_skipped() {
    let mapping = Mapping {
        skip_rows: 2,
        ..signed_mapping()
    };
    let text = "Account,0012345\n\
                Period,Nov 2026\n\
                Date,Description,Amount,Balance\n\
                03/11/2026,Trotro,-5.00,95.00\n";
    let statement = csv::read(text.as_bytes(), &mapping, GHS).unwrap();

    assert_eq!(statement.lines.len(), 1);
    assert_eq!(statement.lines[0].line_no, 4);
}

#[test]
fn a_line_that_will_not_read_is_reported_and_the_rest_still_come_in() {
    let statement = read(
        "Date,Description,Amount,Balance\n\
         03/11/2026,Trotro,-5.00,95.00\n\
         Closing balance,,,95.00\n\
         04/11/2026,Kenkey,twelve,83.00\n\
         05/11/2026,Nothing,0.00,83.00\n\
         06/11/2026,Short\n\
         07/11/2026,Waakye,-10.00,73.00\n",
    );

    assert_eq!(statement.lines.len(), 2);
    let problems: Vec<(u32, Problem)> = statement
        .problems
        .into_iter()
        .map(|p| (p.line_no, p.problem))
        .collect();
    assert_eq!(
        problems,
        vec![
            (
                3,
                Problem::BadDate {
                    text: "Closing balance".to_owned()
                }
            ),
            (
                4,
                Problem::BadAmount {
                    text: "twelve".to_owned()
                }
            ),
            (5, Problem::ZeroAmount),
            (6, Problem::MissingColumn { column: 2 }),
        ]
    );
}

#[test]
fn an_amount_in_both_split_columns_is_refused_not_netted() {
    let mapping = Mapping {
        amount: AmountColumns::Split {
            money_in: 3,
            money_out: 2,
        },
        balance: None,
        ..signed_mapping()
    };
    let statement = csv::read(
        b"Date,Narration,Debit,Credit\n03/11/2026,Odd,5.00,5.00\n03/11/2026,Blank,,\n",
        &mapping,
        GHS,
    )
    .unwrap();

    let problems: Vec<Problem> = statement.problems.into_iter().map(|p| p.problem).collect();
    assert_eq!(problems, vec![Problem::BothAmounts, Problem::NoAmount]);
}

#[test]
fn a_missing_description_keeps_the_money_and_names_the_line() {
    let statement = read("Date,Description,Amount,Balance\n03/11/2026,  ,-5.00,\n");

    assert_eq!(statement.lines[0].label, "Statement line 2");
    assert_eq!(statement.lines[0].reported_balance, None);
}

#[test]
fn a_windows_export_that_is_not_utf8_still_reads() {
    let mut bytes = b"Date,Description,Amount,Balance\n03/11/2026,Caf".to_vec();
    bytes.push(0xE9); // Latin-1 é
    bytes.extend_from_slice(b",-5.00,\n");
    let statement = csv::read(&bytes, &signed_mapping(), GHS).unwrap();

    assert_eq!(statement.lines[0].label, "Café");
}

#[test]
fn a_byte_order_mark_does_not_become_part_of_the_first_cell() {
    let preview = csv::preview(b"\xEF\xBB\xBFDate,Amount\n03/11/2026,5\n", None).unwrap();

    assert_eq!(preview.rows[0][0], "Date");
}

#[test]
fn the_delimiter_is_guessed_from_the_lines_that_agree() {
    let semicolons = "Date;Description;Amount\n03/11/2026;Shop, Osu;-5,00\n04/11/2026;Fare;-2\n";
    assert_eq!(csv::sniff(semicolons), Delimiter::Semicolon);
    assert_eq!(csv::sniff("a\tb\tc\n1\t2\t3\n"), Delimiter::Tab);
    assert_eq!(csv::sniff("just one column\n"), Delimiter::Comma);
}

#[test]
fn a_preview_splits_the_first_rows_into_cells() {
    let preview = csv::preview(b"Date,Description\n03/11/2026,\"Shop, Osu\"\n", None).unwrap();

    assert_eq!(preview.delimiter, Delimiter::Comma);
    assert_eq!(
        preview.rows,
        vec![
            vec!["Date".to_owned(), "Description".to_owned()],
            vec!["03/11/2026".to_owned(), "Shop, Osu".to_owned()],
        ]
    );
}

#[test]
fn a_mapping_that_cannot_describe_a_statement_is_refused_before_reading() {
    let same_columns = Mapping {
        amount: AmountColumns::Split {
            money_in: 2,
            money_out: 2,
        },
        ..signed_mapping()
    };
    let date_is_amount = Mapping {
        amount: AmountColumns::Signed { column: 0 },
        ..signed_mapping()
    };
    let too_wide = Mapping {
        label: 999,
        ..signed_mapping()
    };

    for mapping in [same_columns, date_is_amount, too_wide] {
        assert!(matches!(
            csv::read(b"a\n", &mapping, GHS),
            Err(IngestError::BadMapping { .. })
        ));
    }
}

#[test]
fn empty_and_oversized_files_are_refused_whole() {
    assert_eq!(
        csv::read(b"  \n\n", &signed_mapping(), GHS),
        Err(IngestError::Empty)
    );
    assert_eq!(
        csv::read(b"Date,Description,Amount\n", &signed_mapping(), GHS),
        Err(IngestError::Empty)
    );
    let huge = vec![b'a'; MAX_STATEMENT_BYTES + 1];
    assert!(matches!(
        csv::read(&huge, &signed_mapping(), GHS),
        Err(IngestError::TooLarge { .. })
    ));
}

#[test]
fn a_file_with_more_rows_than_one_review_can_take_is_refused() {
    let mut text = String::from("Date,Description,Amount,Balance\n");
    for _ in 0..=MAX_STATEMENT_LINES {
        text.push_str("03/11/2026,Fare,-1.00,\n");
    }

    assert_eq!(
        csv::read(text.as_bytes(), &signed_mapping(), GHS),
        Err(IngestError::TooManyLines {
            limit: MAX_STATEMENT_LINES
        })
    );
}

#[test]
fn a_mapping_round_trips_through_json_as_the_server_stores_it() {
    let mapping = signed_mapping();
    let json = serde_json::to_string(&mapping).unwrap();

    assert!(json.contains("\"kind\":\"signed\""), "{json}");
    assert!(
        json.contains("\"date_format\":\"day_month_year\""),
        "{json}"
    );
    assert_eq!(serde_json::from_str::<Mapping>(&json).unwrap(), mapping);
}

// --------------------------------------------------------------------------------- ofx ----

const OFX_SGML: &str = "OFXHEADER:100\nDATA:OFXSGML\nVERSION:102\n\n\
<OFX><BANKMSGSRSV1><STMTTRNRS><STMTRS><CURDEF>GHS\n\
<BANKACCTFROM><ACCTID>0012345</BANKACCTFROM>\n\
<BANKTRANLIST><DTSTART>20261101\n\
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20261103120000[0:GMT]<TRNAMT>-20.00<FITID>A1<NAME>SHOPRITE ACCRA MALL<MEMO>POS\n\
<STMTTRN><TRNTYPE>CREDIT<DTPOSTED>20261104<TRNAMT>500<FITID>A2<MEMO>Salary &amp; allowances\n\
</BANKTRANLIST></STMTRS></STMTTRNRS></BANKMSGSRSV1></OFX>\n";

#[test]
fn sgml_ofx_reads_with_unclosed_elements() {
    let statement = ofx::read(OFX_SGML.as_bytes(), GHS).unwrap();

    assert!(statement.problems.is_empty(), "{:?}", statement.problems);
    let [shop, salary] = statement.lines.as_slice() else {
        panic!("expected two lines, got {:?}", statement.lines);
    };
    assert_eq!(shop.direction, Direction::Out);
    assert_eq!(shop.amount, Money::new(2_000, GHS));
    assert_eq!(shop.booked_on, date!(2026 - 11 - 03));
    assert_eq!(shop.label, "SHOPRITE ACCRA MALL");
    assert_eq!(shop.counterparty.as_deref(), Some("SHOPRITE ACCRA MALL"));
    assert_eq!(shop.external_id.as_deref(), Some("A1"));
    assert_eq!(salary.direction, Direction::In);
    assert_eq!(salary.label, "Salary & allowances");
    assert_eq!(salary.counterparty, None);
    assert_eq!((shop.line_no, salary.line_no), (1, 2));
}

#[test]
fn xml_ofx_reads_with_closed_elements() {
    let text = "<?xml version=\"1.0\"?><?OFX OFXHEADER=\"200\"?>\
<OFX><CREDITCARDMSGSRSV1><CCSTMTTRNRS><CCSTMTRS><CURDEF>USD</CURDEF>\
<BANKTRANLIST>\
<STMTTRN><TRNTYPE>DEBIT</TRNTYPE><DTPOSTED>20261103</DTPOSTED><TRNAMT>-9,99</TRNAMT>\
<FITID>X-1</FITID><NAME>Streaming</NAME></STMTTRN>\
</BANKTRANLIST></CCSTMTRS></CCSTMTTRNRS></CREDITCARDMSGSRSV1></OFX>";
    let statement = ofx::read(text.as_bytes(), USD).unwrap();

    assert_eq!(statement.lines.len(), 1);
    assert_eq!(statement.lines[0].amount, Money::new(999, USD));
    assert_eq!(statement.lines[0].direction, Direction::Out);
}

#[test]
fn an_ofx_movement_that_will_not_read_is_reported_by_position() {
    let text = "<OFX><STMTRS><CURDEF>GHS<BANKTRANLIST>\
<STMTTRN><DTPOSTED>20261103<TRNAMT>-1.00<NAME>Fine\
<STMTTRN><DTPOSTED>2026-11<TRNAMT>-1.00\
<STMTTRN><DTPOSTED>20261103<TRNAMT>lots\
<STMTTRN><TRNAMT>-1.00\
</BANKTRANLIST></STMTRS></OFX>";
    let statement = ofx::read(text.as_bytes(), GHS).unwrap();

    assert_eq!(statement.lines.len(), 1);
    let positions: Vec<u32> = statement.problems.iter().map(|p| p.line_no).collect();
    assert_eq!(positions, vec![2, 3, 4]);
    assert!(matches!(
        statement.problems[1].problem,
        Problem::BadAmount { .. }
    ));
}

#[test]
fn an_ofx_statement_in_another_currency_is_refused_whole() {
    assert_eq!(
        ofx::read(OFX_SGML.as_bytes(), USD),
        Err(IngestError::CurrencyMismatch {
            statement: "GHS".to_owned(),
            account: USD,
        })
    );
}

#[test]
fn an_ofx_file_holding_two_accounts_is_refused() {
    let text = "<OFX><STMTRS><CURDEF>GHS</STMTRS><STMTRS><CURDEF>GHS</STMTRS></OFX>";

    assert_eq!(
        ofx::read(text.as_bytes(), GHS),
        Err(IngestError::SeveralStatements { count: 2 })
    );
}

#[test]
fn a_file_that_is_not_ofx_is_said_to_be_so() {
    assert_eq!(
        ofx::read(b"Date,Amount\n03/11/2026,5\n", GHS),
        Err(IngestError::NotOfx)
    );
}

// ------------------------------------------------------------------------------- stage ----

const ACCRA: UtcOffset = offset!(UTC);

#[test]
fn staging_ties_each_line_to_the_account_at_the_start_of_its_day() {
    let account: AccountId = id();
    let statement = read("Date,Description,Amount,Balance\n03/11/2026,Trotro,-5.00,\n");
    let staged = stage(&statement, account, &offset!(+1)).staged;

    let candidate = &staged[0].candidate;
    assert_eq!(candidate.account_id, Some(account));
    assert_eq!(candidate.booked_at, datetime!(2026-11-02 23:00:00 UTC));
    assert_eq!(candidate.category_id, None);
    assert!(candidate.validate().is_ok());
    assert_eq!(staged[0].fingerprint.len(), 64);
}

#[test]
fn the_same_statement_staged_twice_gives_the_same_fingerprints() {
    let account: AccountId = id();
    let text =
        "Date,Description,Amount,Balance\n03/11/2026,Trotro,-5.00,\n04/11/2026,Kenkey,-8.00,\n";
    let first = stage(&read(text), account, &ACCRA).staged;
    let second = stage(&read(text), account, &ACCRA).staged;

    let prints = |staged: &[mt_core::ingest::Staged]| -> Vec<String> {
        staged.iter().map(|s| s.fingerprint.clone()).collect()
    };
    assert_eq!(prints(&first), prints(&second));
}

#[test]
fn two_identical_fares_on_one_day_are_two_fingerprints_not_one() {
    let account: AccountId = id();
    let text =
        "Date,Description,Amount,Balance\n03/11/2026,Trotro,-5.00,\n03/11/2026,Trotro,-5.00,\n";
    let staged = stage(&read(text), account, &ACCRA).staged;

    assert_ne!(staged[0].fingerprint, staged[1].fingerprint);
}

#[test]
fn the_same_line_in_another_account_is_not_the_same_line() {
    let text = "Date,Description,Amount,Balance\n03/11/2026,Trotro,-5.00,\n";
    let one = stage(&read(text), id(), &ACCRA).staged;
    let other = stage(&read(text), id(), &ACCRA).staged;

    assert_ne!(one[0].fingerprint, other[0].fingerprint);
}

// ------------------------------------------------------------- what review turned up ----

#[test]
fn a_zero_in_the_unused_split_column_is_read_as_blank() {
    let mapping = Mapping {
        amount: AmountColumns::Split {
            money_in: 3,
            money_out: 2,
        },
        balance: None,
        ..signed_mapping()
    };
    let statement = csv::read(
        b"Date,Narration,Debit,Credit\n\
          03/11/2026,ATM,50.00,0.00\n\
          04/11/2026,Salary,0.00,200.00\n\
          05/11/2026,Nothing,0.00,0.00\n",
        &mapping,
        GHS,
    )
    .unwrap();

    let moves: Vec<(Direction, i64)> = statement
        .lines
        .iter()
        .map(|line| (line.direction, line.amount.minor()))
        .collect();
    assert_eq!(
        moves,
        vec![(Direction::Out, 5_000), (Direction::In, 20_000)]
    );
    let problems: Vec<Problem> = statement.problems.into_iter().map(|p| p.problem).collect();
    assert_eq!(problems, vec![Problem::NoAmount]);
}

#[test]
fn a_balance_too_large_to_be_one_is_a_line_problem_not_a_failed_upload() {
    let statement = read(
        "Date,Description,Amount,Balance\n\
         03/11/2026,Card,-5.00,4111111111111111\n\
         04/11/2026,Fare,-2.00,93.00\n",
    );

    assert_eq!(statement.lines.len(), 1);
    assert_eq!(statement.problems[0].problem, Problem::AmountTooLarge);
}

#[test]
fn a_year_no_ledger_can_hold_is_a_line_problem() {
    let statement = read(
        "Date,Description,Amount,Balance\n\
         03/11/0226,Typo,-5.00,\n\
         04/11/2026,Fare,-2.00,\n",
    );

    assert_eq!(statement.lines.len(), 1);
    assert!(matches!(
        statement.problems[0].problem,
        Problem::BadDate { .. }
    ));
}

#[test]
fn an_amount_in_another_currency_is_refused_not_read_as_cedis() {
    let statement = read(
        "Date,Description,Amount,Balance\n\
         03/11/2026,Netflix,USD -15.99,\n\
         04/11/2026,Fare,GHS -2.00,\n",
    );

    assert_eq!(statement.lines.len(), 1);
    assert!(matches!(
        statement.problems[0].problem,
        Problem::BadAmount { .. }
    ));
}

#[test]
fn an_ofx_movement_in_another_currency_is_refused() {
    let text = "<OFX><STMTRS><CURDEF>GHS<BANKTRANLIST>\
<STMTTRN><DTPOSTED>20261103<TRNAMT>-9.99<NAME>Streaming<CURRENCY><CURRATE>15<CURSYM>USD</CURRENCY>\
<STMTTRN><DTPOSTED>20261103<TRNAMT>-2.00<NAME>Fare\
</BANKTRANLIST></STMTRS></OFX>";
    let statement = ofx::read(text.as_bytes(), GHS).unwrap();

    assert_eq!(statement.lines.len(), 1);
    assert!(matches!(
        statement.problems[0].problem,
        Problem::OtherCurrency { .. }
    ));
}

#[test]
fn a_reference_shared_by_lines_of_one_statement_is_kept_by_none_of_them() {
    let mapping = Mapping {
        reference: Some(3),
        balance: None,
        ..signed_mapping()
    };
    let statement = csv::read(
        b"Date,Description,Amount,Ref\n\
          03/11/2026,Transfer to Kofi,-500.00,TRF-1\n\
          03/11/2026,E-levy,-5.00,TRF-1\n\
          04/11/2026,Fare,-2.00,F-9\n",
        &mapping,
        GHS,
    )
    .unwrap();
    let staged = stage(&statement, id(), &ACCRA).staged;

    let references: Vec<Option<&str>> = staged
        .iter()
        .map(|s| s.candidate.external_id.as_deref())
        .collect();
    assert_eq!(references, vec![None, None, Some("F-9")]);
}

#[test]
fn adding_a_column_to_the_mapping_does_not_offer_the_statement_again() {
    let account: AccountId = id();
    let text = "Date,Description,Amount,Balance\n03/11/2026,Trotro,-5.00,95.00\n";
    let without_balance = Mapping {
        balance: None,
        ..signed_mapping()
    };
    let first = stage(
        &csv::read(text.as_bytes(), &without_balance, GHS).unwrap(),
        account,
        &ACCRA,
    )
    .staged;
    let second = stage(&read(text), account, &ACCRA).staged;

    assert_eq!(first[0].fingerprint, second[0].fingerprint);
}

#[test]
fn a_line_that_would_not_make_a_valid_row_is_left_out_by_staging() {
    let mut statement = read("Date,Description,Amount,Balance\n03/11/2026,Fare,-2.00,\n");
    let mut bad = statement.lines[0].clone();
    bad.line_no = 3;
    bad.reported_balance = Some(Money::new(5, USD));
    statement.lines.push(bad);

    let staging = stage(&statement, id(), &ACCRA);

    assert_eq!(staging.staged.len(), 1);
    assert_eq!(staging.problems[0].line_no, 3);
}
