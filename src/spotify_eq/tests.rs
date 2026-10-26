use super::*;

const PRESETS: &str = r#"[{"name":"Low/High 2:1","filters":[{"frequency":60,"key":"audio.equalizer.low_shelf_gain_v2","type":"lowshelf","gain":5.1},{"frequency":150,"key":"audio.equalizer.low_peak_gain_v2","type":"peaking","gain":2.5},{"frequency":400,"key":"audio.equalizer.low_mid_peak_gain_v2","type":"peaking","gain":0},{"frequency":1000,"key":"audio.equalizer.high_mid_peak_gain_v2","type":"peaking","gain":0},{"frequency":2400,"key":"audio.equalizer.high_peak_gain_v2","type":"peaking","gain":0},{"frequency":15000,"key":"audio.equalizer.high_shelf_gain_v2","type":"highshelf","gain":2.7}]}]"#;

fn varint(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

fn key(user: &str) -> Vec<u8> {
    let mut key = ORIGIN.to_vec();
    key.extend_from_slice(user.as_bytes());
    key.extend_from_slice(b":eq_custom_presets");
    key
}

fn latin1(text: &str) -> Vec<u8> {
    let mut value = vec![1];
    value.extend_from_slice(text.as_bytes());
    value
}

/// Snappy as literals only, which every decoder reads.
fn snappy_literal(data: &[u8]) -> Vec<u8> {
    let mut out = varint(data.len() as u64);
    for chunk in data.chunks(60_000) {
        let n = chunk.len() - 1;
        if n < 60 {
            out.push((n as u8) << 2);
        } else if n < 256 {
            out.extend_from_slice(&[60 << 2, n as u8]);
        } else {
            out.extend_from_slice(&[61 << 2, n as u8, (n >> 8) as u8]);
        }
        out.extend_from_slice(chunk);
    }
    out
}

/// A block whose keys share their prefixes with the key before, as
/// LevelDB writes them.
fn block_bytes(entries: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut previous: &[u8] = &[];
    for (key, value) in entries {
        let shared = previous.iter().zip(key).take_while(|(a, b)| a == b).count();
        out.extend(varint(shared as u64));
        out.extend(varint((key.len() - shared) as u64));
        out.extend(varint(value.len() as u64));
        out.extend_from_slice(&key[shared..]);
        out.extend_from_slice(value);
        previous = key;
    }
    out.extend(0u32.to_le_bytes());
    out.extend(1u32.to_le_bytes());
    out
}

/// A user key, its sequence, and its value or `None` for a deletion.
type Written = (Vec<u8>, u64, Option<Vec<u8>>);
/// A batch's sequence and its keys with their values or deletions.
type Batch = (u64, Vec<(Vec<u8>, Option<Vec<u8>>)>);

/// A table of records in one data block.
fn table(records: &[Written], compressed: bool) -> Vec<u8> {
    let entries: Vec<(Vec<u8>, Vec<u8>)> = records
        .iter()
        .map(|(key, sequence, value)| {
            let mut internal = key.clone();
            let kind = u64::from(value.is_some());
            internal.extend((sequence << 8 | kind).to_le_bytes());
            (internal, value.clone().unwrap_or_default())
        })
        .collect();
    let raw = block_bytes(&entries);
    let (contents, kind) = if compressed {
        (snappy_literal(&raw), 1u8)
    } else {
        (raw, 0u8)
    };
    let mut out = contents.clone();
    out.push(kind);
    out.extend([0u8; 4]);
    let mut handle = varint(0);
    handle.extend(varint(contents.len() as u64));
    let index = block_bytes(&[(b"z".to_vec(), handle)]);
    let index_at = out.len();
    out.extend_from_slice(&index);
    out.push(0);
    out.extend([0u8; 4]);
    let mut footer = varint(0);
    footer.extend(varint(0));
    footer.extend(varint(index_at as u64));
    footer.extend(varint(index.len() as u64));
    footer.resize(40, 0);
    footer.extend(0xdb47_7524_8b80_fb57u64.to_le_bytes());
    out.extend(footer);
    out
}

/// A log of one batch per record, each split across blocks as LevelDB
/// writes it.
fn log(batches: &[Batch]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for (sequence, records) in batches {
        let mut batch = sequence.to_le_bytes().to_vec();
        batch.extend((records.len() as u32).to_le_bytes());
        for (key, value) in records {
            batch.push(u8::from(value.is_some()));
            batch.extend(varint(key.len() as u64));
            batch.extend_from_slice(key);
            if let Some(value) = value {
                batch.extend(varint(value.len() as u64));
                batch.extend_from_slice(value);
            }
        }
        let mut rest: &[u8] = &batch;
        let mut first = true;
        loop {
            let left = 32 * 1024 - out.len() % (32 * 1024);
            if left < 7 {
                out.resize(out.len() + left, 0);
                continue;
            }
            let take = rest.len().min(left - 7);
            let last = take == rest.len();
            let kind = match (first, last) {
                (true, true) => 1,
                (true, false) => 2,
                (false, false) => 3,
                (false, true) => 4,
            };
            out.extend([0u8; 4]);
            out.extend((take as u16).to_le_bytes());
            out.push(kind);
            out.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            first = false;
            if last {
                break;
            }
        }
    }
    out
}

/// The preset this computer's Spotify app saved, as it stores it, comes
/// over by name with its curve on the sliders: the low shelf and the
/// 150 Hz peak lift the bass, the high shelf the top, the middle stays.
#[test]
fn spotifys_saved_preset_becomes_sliders() {
    let presets = parse_presets(&latin1(PRESETS)).unwrap();
    let [preset] = presets.as_slice() else {
        panic!("{presets:?}")
    };
    assert_eq!(preset.name, "Low/High 2:1");
    assert_eq!(preset.preamp_db, 0.0);
    let bands = preset.bands_db;
    // A shelf gives half its gain at its own frequency, the peak the rest.
    assert!(bands[0] >= 2.5 && bands[1] > 1.5, "{bands:?}");
    assert!(bands[4].abs() < 0.5 && bands[5].abs() < 0.5, "{bands:?}");
    assert!(bands[9] > 1.5, "{bands:?}");
}

/// Chromium stores text as Latin-1 or as UTF-16; both read, unknown
/// filters are left out, nameless presets are skipped, and gains past
/// the range stop at it.
#[test]
fn preset_text_reads_in_either_encoding() {
    let text = r#"[{"name":"Früh","filters":[{"frequency":1000,"type":"peaking","gain":40},{"frequency":10,"type":"notch","gain":5}]},{"name":"  ","filters":[]}]"#;
    let mut utf16 = vec![0];
    for unit in text.encode_utf16() {
        utf16.extend(unit.to_le_bytes());
    }
    let presets = parse_presets(&utf16).unwrap();
    assert_eq!(presets.len(), 1);
    assert_eq!(presets[0].name, "Früh");
    assert_eq!(presets[0].bands_db[4], crate::eq::RANGE_DB);
    assert_eq!(parse_presets(&latin1("[]")).unwrap(), []);
    assert!(parse_presets(&latin1("{")).is_err());
    assert!(parse_presets(&[7, b'[', b']']).is_err());
    assert!(parse_presets(&[]).is_err());
}

#[test]
fn snappy_copies_and_literals_decompress() {
    // "abc", then six bytes copied from three back.
    const ANY: usize = 1 << 20;
    let data = [9, 2 << 2, b'a', b'b', b'c', (2 << 2) | 1, 3];
    assert_eq!(snappy(&data, ANY).unwrap(), b"abcabcabc");
    // "abcd", then copies with two- and four-byte offsets.
    let wide = [
        12,
        3 << 2,
        b'a',
        b'b',
        b'c',
        b'd',
        (3 << 2) | 2,
        4,
        0,
        (3 << 2) | 3,
        8,
        0,
        0,
        0,
    ];
    assert_eq!(snappy(&wide, ANY).unwrap(), b"abcdabcdabcd");
    let long: Vec<u8> = (0..300u16).map(|n| n as u8).collect();
    assert_eq!(snappy(&snappy_literal(&long), ANY).unwrap(), long);
    // Copies from before the start, lengths that lie, cut-off input, and
    // more than the limit are refused, never read past.
    assert!(snappy(&[3, (2 << 2) | 1, 9], ANY).is_err());
    assert!(snappy(&[9, 2 << 2, b'a', b'b', b'c'], ANY).is_err());
    assert!(snappy(&[2, 2 << 2, b'a'], ANY).is_err());
    assert!(snappy(&[1, (2 << 2) | 1, 0], ANY).is_err());
    assert!(
        snappy(&data, 8).is_err(),
        "nine bytes are past a limit of eight"
    );
}

/// A table finds its records, plain or compressed, by the exact key.
#[test]
fn tables_hold_the_newest_records_for_a_key() {
    let mine = key("me");
    let theirs = key("them");
    for compressed in [false, true] {
        let data = table(
            &[
                (mine.clone(), 7, Some(latin1("[]"))),
                (theirs.clone(), 9, Some(latin1("x"))),
            ],
            compressed,
        );
        assert_eq!(
            table_records(&data, &mine).unwrap(),
            Some((7, Some(latin1("[]"))))
        );
        assert_eq!(table_records(&data, b"nothing").unwrap(), None);
        // A deletion newer than the value is what the table holds.
        let deleted = table(
            &[
                (mine.clone(), 7, Some(latin1("[]"))),
                (mine.clone(), 8, None),
            ],
            compressed,
        );
        assert_eq!(table_records(&deleted, &mine).unwrap(), Some((8, None)));
        assert!(table_records(&data[..data.len() - 1], &mine).is_err());
        assert!(table_records(&data[1..], &mine).is_err());
    }
}

/// A log's batches, even one split over blocks, give their records with
/// each one's own sequence; a deletion is a record without a value.
#[test]
fn logs_give_each_record_its_sequence() {
    let mine = key("me");
    let big = latin1(&format!("[{}]", " ".repeat(40_000)));
    let data = log(&[
        (
            20,
            vec![
                (b"other".to_vec(), Some(b"1".to_vec())),
                (mine.clone(), Some(big.clone())),
            ],
        ),
        (30, vec![(mine.clone(), None)]),
    ]);
    assert!(data.len() > 32 * 1024, "the first batch spans two blocks");
    assert_eq!(log_records(&data, &mine).unwrap(), Some((30, None)));
    let written = log(&[(20, vec![(mine.clone(), Some(big.clone()))])]);
    assert_eq!(log_records(&written, &mine).unwrap(), Some((20, Some(big))));
    // A log cut off mid-write gives what it holds whole.
    assert_eq!(log_records(&data[..100], &mine).unwrap(), None);
}

/// Across a database's files, the newest record wins: a later write in
/// the log over an older table, and a deletion over both.
#[test]
fn the_newest_record_across_files_wins() {
    let dir = std::env::temp_dir().join(format!("spotifast-spotify-eq-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mine = key("me");
    std::fs::write(
        dir.join("000005.ldb"),
        table(&[(mine.clone(), 4, Some(latin1("old")))], true),
    )
    .unwrap();
    std::fs::write(
        dir.join("000009.log"),
        log(&[(8, vec![(mine.clone(), Some(latin1("new")))])]),
    )
    .unwrap();
    std::fs::write(dir.join("LOG"), b"not a table").unwrap();
    std::fs::write(dir.join("000006.ldb"), b"damaged").unwrap();
    assert_eq!(latest_value(&dir, &mine).unwrap(), Some(latin1("new")));
    std::fs::write(
        dir.join("000010.log"),
        log(&[(12, vec![(mine.clone(), None)])]),
    )
    .unwrap();
    assert_eq!(latest_value(&dir, &mine).unwrap(), None);
    assert_eq!(latest_value(&dir, &key("them")).unwrap(), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Fragments with no first one before them, the end of a write the log
/// no longer holds, are passed over: a whole record after them still reads.
#[test]
fn orphan_log_fragments_are_passed_over() {
    let mine = key("me");
    let whole = log(&[(5, vec![(mine.clone(), Some(latin1("[]")))])]);
    let mut data = Vec::new();
    for kind in [4u8, 3, 4] {
        data.extend([0u8; 4]);
        data.extend(9u16.to_le_bytes());
        data.push(kind);
        data.extend(b"junkjunk!");
    }
    data.extend(&whole);
    assert_eq!(
        log_records(&data, &mine).unwrap(),
        Some((5, Some(latin1("[]"))))
    );
}

/// Crafted tables cost no more than their bounds: a long key shared by a
/// great many empty entries is read in place, an index naming one block
/// again and again reads it once, and blocks that decompress past the
/// table's budget are refused.
#[test]
fn crafted_tables_stay_within_bounds() {
    let mine = key("me");
    // One 64 KiB key, then 200,000 entries sharing all of it.
    let long = vec![b'k'; 64 << 10];
    let mut raw = Vec::new();
    raw.extend(varint(0));
    raw.extend(varint(long.len() as u64 + 8));
    raw.extend(varint(0));
    raw.extend(&long);
    raw.extend([1u8; 8]);
    let shared = varint(long.len() as u64 + 8);
    for _ in 0..200_000 {
        raw.extend(&shared);
        raw.extend([0, 0]);
    }
    raw.extend(0u32.to_le_bytes());
    let mut seen = 0usize;
    each_entry(&raw, |key, _| {
        seen += 1;
        assert_eq!(key.len(), long.len() + 8);
        Ok(())
    })
    .unwrap();
    assert_eq!(seen, 200_001);

    // An index naming the same block ten thousand times.
    let mut internal = mine.clone();
    internal.extend((3u64 << 8 | 1).to_le_bytes());
    let block = block_bytes(&[(internal, latin1("[]"))]);
    let mut data = block.clone();
    data.push(0);
    data.extend([0u8; 4]);
    let mut handle = varint(0);
    handle.extend(varint(block.len() as u64));
    let index_entries: Vec<(Vec<u8>, Vec<u8>)> = (0..10_000u32)
        .map(|n| (n.to_be_bytes().to_vec(), handle.clone()))
        .collect();
    let index = block_bytes(&index_entries);
    let index_at = data.len();
    data.extend(&index);
    data.push(0);
    data.extend([0u8; 4]);
    let mut footer = varint(0);
    footer.extend(varint(0));
    footer.extend(varint(index_at as u64));
    footer.extend(varint(index.len() as u64));
    footer.resize(40, 0);
    footer.extend(0xdb47_7524_8b80_fb57u64.to_le_bytes());
    data.extend(footer);
    assert_eq!(
        table_records(&data, &mine).unwrap(),
        Some((3, Some(latin1("[]"))))
    );

    // A block claiming to decompress past the budget.
    let mut budget = 16;
    let huge = [0x80, 0x80, 0x80, 0x10, 0];
    let mut file = huge.to_vec();
    file.push(1);
    file.extend([0u8; 4]);
    assert!(super::block(&file, 0, huge.len() as u64, &mut budget).is_err());
}
