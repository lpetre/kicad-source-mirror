// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at your
// option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Port of `qa/tests/common/test_kiid.cpp` (Boost test suite `Kiid`), followed by additional
//! tests pinning the behaviour of the Boost.Uuid pieces `KIID` is built on.

use std::collections::BTreeSet;

use kicad_core::kiid::{
    Kiid, KiidGenerator, KiidNilSetReset, KiidPath, NILUUID, Timestamp, nil_uuid,
};

// ---- qa/tests/common/test_kiid.cpp ----

#[test]
fn seeding() {
    let mut generator = KiidGenerator::seeded(0);

    let a0 = Kiid::new(&mut generator);
    let b0 = Kiid::new(&mut generator);
    let c0 = Kiid::new(&mut generator);
    let d0 = Kiid::new(&mut generator);

    // Make sure the hashes are unique
    let mut hashes = BTreeSet::new();
    hashes.insert(a0.hash_value());
    hashes.insert(b0.hash_value());
    hashes.insert(c0.hash_value());
    hashes.insert(d0.hash_value());
    assert_eq!(hashes.len(), 4);

    generator.seed_generator(0);

    let a = Kiid::new(&mut generator);
    assert_eq!(a.hash_value(), a0.hash_value());

    let b = Kiid::new(&mut generator);
    assert_eq!(b.hash_value(), b0.hash_value());

    let c = Kiid::new(&mut generator);
    assert_eq!(c.hash_value(), c0.hash_value());

    let d = Kiid::new(&mut generator);
    assert_eq!(d.hash_value(), d0.hash_value());
}

#[test]
fn kiid_path_test() {
    let mut generator = KiidGenerator::default();

    let a = Kiid::new(&mut generator);
    let b = Kiid::new(&mut generator);
    let c = Kiid::new(&mut generator);
    let d = Kiid::new(&mut generator);

    let mut path1 = KiidPath::new();
    let mut path2 = KiidPath::new();

    path1.push(a);
    path1.push(b);
    path1.push(c);
    path1.push(d);

    path2.push(b);
    path2.push(c);
    path2.push(d);

    assert!(path1.ends_with(&path2));
    assert!(!path2.ends_with(&path1));
}

#[test]
fn legacy_timestamp() {
    let ts_a: Timestamp = 0xAABBCCDD;
    let ts_b: Timestamp = 0x00000012;

    let str_a = "AABBCCDD";
    let str_b = "00000012";

    let a = Kiid::from_legacy_timestamp(ts_a);
    let b = Kiid::from_legacy_timestamp(ts_b);

    assert!(a.as_legacy_timestamp() == ts_a);
    assert!(a.as_legacy_timestamp_string() == str_a);

    assert!(b.as_legacy_timestamp() == ts_b);
    assert!(b.as_legacy_timestamp_string() == str_b);

    let mut generator = KiidGenerator::default();

    assert!(Kiid::from_string_or_random(str_a, &mut generator).as_legacy_timestamp() == ts_a);
    assert!(Kiid::from_string_or_random(str_b, &mut generator).as_legacy_timestamp() == ts_b);

    assert!(
        Kiid::from_string_or_random(str_a, &mut generator).as_legacy_timestamp_string() == str_a
    );
    assert!(
        Kiid::from_string_or_random(str_b, &mut generator).as_legacy_timestamp_string() == str_b
    );
}

#[test]
fn combine_deterministic() {
    let mut generator = KiidGenerator::default();

    let a = Kiid::new(&mut generator);
    let b = Kiid::new(&mut generator);
    let c = Kiid::new(&mut generator);

    // Combining the same two KIIDs should always produce the same result
    let combined1 = Kiid::combine(&a, &b);
    let combined2 = Kiid::combine(&a, &b);
    assert_eq!(combined1.as_string(), combined2.as_string());

    // Combined result should be different from inputs
    assert!(combined1 != a);
    assert!(combined1 != b);

    // Different inputs should produce different outputs
    let combined3 = Kiid::combine(&a, &c);
    assert!(combined1 != combined3);

    // Order matters for XOR with different values
    let combined4 = Kiid::combine(&b, &a);
    assert_eq!(combined1.as_string(), combined4.as_string());
}

#[test]
fn byte_round_trip() {
    let id = Kiid::parse("01234567-89ab-cdef-0123-456789abcdef").unwrap();

    assert!(Kiid::from_bytes(id.as_bytes()) == id);
}

#[test]
fn combine_with_known_values() {
    // Test with known UUID strings to verify XOR behavior
    let a = Kiid::parse("00000000-0000-0000-0000-000000000001").unwrap();
    let b = Kiid::parse("00000000-0000-0000-0000-000000000002").unwrap();

    let combined = Kiid::combine(&a, &b);

    // XOR of 0x01 and 0x02 = 0x03
    assert_eq!(combined.as_string(), "00000000-0000-0000-0000-000000000003");

    // Combining with self should give all zeros
    let self_combined = Kiid::combine(&a, &a);
    assert_eq!(
        self_combined.as_string(),
        "00000000-0000-0000-0000-000000000000"
    );
}

#[test]
fn combine_uniqueness() {
    // Generate several random KIIDs and verify all combinations are unique.
    // XOR is commutative, so Combine(a,b) == Combine(b,a). We only count
    // unique unordered pairs, which gives n*(n-1)/2 combinations.
    let mut generator = KiidGenerator::default();
    let mut ids = Vec::new();

    for _ in 0..10 {
        ids.push(Kiid::new(&mut generator));
    }

    let mut combinations = BTreeSet::new();

    for i in 0..ids.len() {
        for j in i + 1..ids.len() {
            let combined = Kiid::combine(&ids[i], &ids[j]);
            combinations.insert(combined.as_string());
        }
    }

    // All 45 unique unordered pairs (10*9/2) should produce unique results
    assert_eq!(combinations.len(), 45);
}

#[test]
fn from_deterministic_string_is_stable() {
    // Same input always yields the same UUID.
    assert_eq!(
        Kiid::from_deterministic_string("Device:R").as_string(),
        Kiid::from_deterministic_string("Device:R").as_string()
    );

    // Distinct inputs yield distinct UUIDs.
    assert!(
        Kiid::from_deterministic_string("Device:R") != Kiid::from_deterministic_string("Device:C")
    );
}

#[test]
fn from_deterministic_string_pinned_output() {
    // Pinned literals capture the exact bytes the diff/merge engine persists.
    // These values come from the original LibraryItemKiidPath dual-FNV-1a
    // algorithm and must never change, or item matching against existing
    // diff/merge artifacts silently breaks.
    assert_eq!(
        Kiid::from_deterministic_string("Device:R").as_string(),
        "d645496c-72ea-dfdb-92c1-78b59d2354ae"
    );

    assert_eq!(
        Kiid::from_deterministic_string("R_0402_1005Metric").as_string(),
        "b9495ba4-dc44-bfa3-d088-a4eb656b1fb2"
    );

    assert_eq!(
        Kiid::from_deterministic_string("some known string").as_string(),
        "6cfc0645-11f5-e66f-a194-2648b4eaec33"
    );
}

// ---- additional tests (not in the C++ suite) ----

/// The ids `KIID()` produces after `KIID::SeedGenerator( seed )`, and their `Hash()` values,
/// as dumped from the C++ implementation (Boost 1.83, x86-64 Linux).
#[test]
fn seeded_sequences_match_cpp() {
    let cases: [(u32, [(&str, u64); 4]); 5] = [
        (
            0,
            [
                ("ac0a7f8c-2faa-4497-b5a6-16b7c0cc21d8", 16210025005716955233),
                ("43b34e9a-fb52-42db-8376-7d8b677de5d8", 16029307586301901785),
                ("09a4746c-d3de-419f-9551-59a5f2d66662", 1486788933435781391),
                ("24b70570-573a-4b4c-863c-4be4d8bd840e", 3212971903063924183),
            ],
        ),
        (
            1,
            [
                ("25f4c16a-eb80-47ff-8c2f-67b84814bcee", 6701939266641405877),
                ("ff7e0700-89c3-4c20-8baa-654d851ec1ff", 14840539077078412766),
                ("4fcb9125-c053-403c-9080-a31781508665", 12665714960144098950),
                ("ccbfae2f-471e-4e63-adaa-7658fc7974ab", 17021941924851557031),
            ],
        ),
        (
            5489,
            [
                ("5cbb91d0-f69e-4e22-aefa-e1e7791fc3d5", 4633642649604374872),
                ("2c358220-dfb7-47f8-8500-d3e9e1af9538", 2560392639685965794),
                ("ba4be2a1-2b09-444e-a368-f81825a6168c", 14978788725219860537),
                ("c4a84b47-1acd-4930-9f6d-008c10782dfe", 17683441366773124632),
            ],
        ),
        (
            12345,
            [
                ("e251fbed-e52d-41e3-9dfd-fd5081087621", 5998013282421920046),
                ("a44d152f-a9ad-4d0a-be0f-5e347e5191d3", 1937233789343088378),
                ("656d5691-2242-4688-9d9e-759801bbd0f4", 3088297092635698773),
                ("766ceaf6-71cd-4076-bb9d-36a78e1a7aec", 1981311926233728815),
            ],
        ),
        (
            0xFFFF_FFFF,
            [
                ("a369fe18-2241-421c-8cec-91e947ac0c90", 15711516744849905408),
                ("b437fec9-40fe-4c86-9650-aec7fa0147c5", 16001646198720442668),
                ("d9b19704-207f-4748-bf8a-31f87b721f81", 12122415631780579971),
                ("f256da97-6b38-4cb2-8bef-6725ca2807b3", 1292328422382815613),
            ],
        ),
    ];

    for (seed, expected) in cases {
        let mut generator = KiidGenerator::seeded(seed);

        for (uuid, hash) in expected {
            let id = Kiid::new(&mut generator);
            assert_eq!(id.as_string(), uuid, "seed {seed}");
            assert_eq!(id.hash_value(), hash, "seed {seed}");
            assert!(!id.is_legacy_timestamp());
        }
    }

    // 1000 ids cross several refills of the Mersenne Twister state.
    let mut generator = KiidGenerator::seeded(42);
    let mut id = NILUUID;

    for _ in 0..1000 {
        id = Kiid::new(&mut generator);
    }

    assert_eq!(id.as_string(), "6b239b0e-6992-4a0b-9d69-3d4855254a09");

    // The default generator is the one seeded with the boost::mt19937 default seed.
    assert_eq!(
        Kiid::new(&mut KiidGenerator::default()).as_string(),
        "5cbb91d0-f69e-4e22-aefa-e1e7791fc3d5"
    );
}

#[test]
fn entropy_seeded_generator() {
    // The entropy source is asked once for the whole Mersenne Twister state.
    let mut calls = 0;
    let mut generator = KiidGenerator::from_entropy(|buffer| {
        calls += 1;
        assert_eq!(buffer.len(), KiidGenerator::ENTROPY_BYTES);

        for (i, byte) in buffer.iter_mut().enumerate() {
            *byte = (i * 7 + 3) as u8;
        }
    });

    assert_eq!(calls, 1);

    let a = Kiid::new(&mut generator);
    let b = Kiid::new(&mut generator);
    assert_ne!(a, b);

    // Version 4, RFC 4122 variant.
    for id in [a, b] {
        let bytes = id.as_bytes();
        assert_eq!(bytes[6] & 0xF0, 0x40);
        assert_eq!(bytes[8] & 0xC0, 0x80);
    }

    // Same entropy, same ids; different entropy, different ids.
    let mut same = KiidGenerator::from_entropy(|buffer| {
        for (i, byte) in buffer.iter_mut().enumerate() {
            *byte = (i * 7 + 3) as u8;
        }
    });
    assert_eq!(Kiid::new(&mut same), a);

    let mut other = KiidGenerator::from_entropy(|buffer| buffer.fill(0x5A));
    assert_ne!(Kiid::new(&mut other), a);

    // Even an all-zero entropy buffer gives a working generator.
    let mut zero = KiidGenerator::from_entropy(|buffer| buffer.fill(0));
    let ids: BTreeSet<Kiid> = (0..400).map(|_| Kiid::new(&mut zero)).collect();
    assert!(ids.len() > 1);
}

#[test]
fn nil_uuid_mode() {
    let mut generator = KiidGenerator::seeded(0);
    assert!(!generator.is_creating_nil_uuids());

    generator.create_nil_uuids(true);
    assert_eq!(Kiid::new(&mut generator), NILUUID);
    assert_eq!(generator.generate(), nil_uuid());

    // The fallback of the string constructor and ConvertTimestampToUuid() draw from the
    // random generator even in nil mode.
    let fallback = Kiid::from_string_or_random("not a uuid", &mut generator);
    assert_eq!(fallback.as_string(), "ac0a7f8c-2faa-4497-b5a6-16b7c0cc21d8");

    let mut legacy = Kiid::from_legacy_timestamp(0x1234);
    legacy.convert_timestamp_to_uuid(&mut generator);
    assert_eq!(legacy.as_string(), "43b34e9a-fb52-42db-8376-7d8b677de5d8");

    // A true UUID is left alone.
    let before = legacy;
    legacy.convert_timestamp_to_uuid(&mut generator);
    assert_eq!(legacy, before);

    generator.create_nil_uuids(false);
    assert_eq!(
        Kiid::new(&mut generator).as_string(),
        "09a4746c-d3de-419f-9551-59a5f2d66662"
    );

    // KIID_NIL_SET_RESET
    {
        let mut guard = KiidNilSetReset::new(&mut generator);
        assert!(guard.is_creating_nil_uuids());
        assert_eq!(Kiid::new(&mut guard), NILUUID);
    }

    assert!(!generator.is_creating_nil_uuids());
    assert_eq!(
        Kiid::new(&mut generator).as_string(),
        "24b70570-573a-4b4c-863c-4be4d8bd840e"
    );
}

#[test]
fn string_constructor() {
    let mut generator = KiidGenerator::seeded(1);

    // Legacy timestamps: up to 8 hex digits, right aligned.
    for (text, timestamp) in [
        ("1", 0x1u32),
        ("12", 0x12),
        ("123", 0x123),
        ("1234", 0x1234),
        ("12345", 0x12345),
        ("123456", 0x123456),
        ("1234567", 0x1234567),
        ("12345678", 0x12345678),
        ("abcDEF", 0xABCDEF),
        ("00000000", 0),
    ] {
        let id = Kiid::parse(text).unwrap();
        assert!(id.is_legacy_timestamp(), "{text}");
        assert_eq!(id.as_legacy_timestamp(), timestamp, "{text}");
        assert_eq!(&id.as_bytes()[..12], &[0u8; 12], "{text}");
        assert_eq!(Kiid::from_string_or_random(text, &mut generator), id);
        assert_eq!(text.parse::<Kiid>(), Ok(id));
    }

    // 9 hex digits are neither a timestamp nor a UUID.
    assert_eq!(Kiid::parse("123456789"), None);
    assert_eq!(Kiid::parse(""), None);
    assert_eq!(Kiid::parse("xyz"), None);
    assert!("xyz".parse::<Kiid>().is_err());
    assert_eq!(
        "xyz".parse::<Kiid>().unwrap_err().to_string(),
        "invalid uuid string"
    );

    // ... so the C++ constructor assigns random ids (no id was drawn above).
    assert_eq!(
        Kiid::from_string_or_random("123456789", &mut generator).as_string(),
        "25f4c16a-eb80-47ff-8c2f-67b84814bcee"
    );
    assert_eq!(
        Kiid::from_string_or_random("", &mut generator).as_string(),
        "ff7e0700-89c3-4c20-8baa-654d851ec1ff"
    );

    // UUID strings in all the forms boost::uuids::string_generator accepts.
    let expected = "01234567-89ab-cdef-0123-456789abcdef";

    for text in [
        "01234567-89ab-cdef-0123-456789abcdef",
        "0123456789abcdef0123456789abcdef",
        "{01234567-89ab-cdef-0123-456789abcdef}",
        "{0123456789abcdef0123456789abcdef}",
        "01234567-89AB-CDEF-0123-456789ABCDEF",
    ] {
        assert_eq!(Kiid::parse(text).unwrap().as_string(), expected, "{text}");
    }

    let id = Kiid::parse(expected).unwrap();
    assert!(!id.is_legacy_timestamp());
    assert_eq!(id.as_std_string(), expected);
    assert_eq!(id.to_string(), expected);
    assert_eq!(format!("{id:?}"), format!("Kiid({expected})"));
    assert_eq!(id.as_legacy_timestamp(), 0x89abcdef);
    assert_eq!(<[u8; 16]>::from(id), id.as_bytes());
    assert_eq!(Kiid::from(id.as_bytes()), id);
}

#[test]
fn sniff_test() {
    assert!(Kiid::sniff_test("01234567-89ab-cdef-0123-456789abcdef"));
    assert!(Kiid::sniff_test("01234567-89AB-CDEF-0123-456789ABCDEF"));
    assert!(Kiid::sniff_test(&NILUUID.as_string()));
    // Only the length and the character set are checked.
    assert!(Kiid::sniff_test("------------------------------------"));
    assert!(Kiid::sniff_test("0123456789abcdef0123456789abcdef0123"));

    assert!(!Kiid::sniff_test(""));
    assert!(!Kiid::sniff_test("0123456789abcdef0123456789abcdef"));
    assert!(!Kiid::sniff_test("01234567-89ab-cdef-0123-456789abcdeg"));
    assert!(!Kiid::sniff_test("{01234567-89ab-cdef-0123-456789abcdef}"));
    assert!(!Kiid::sniff_test(
        "01234567-89ab-cdef-0123-456789abcd\u{e9}"
    ));
    assert!(!Kiid::sniff_test("0123456789ab-cdef-0123-456789abcd\u{e9}"));
}

#[test]
fn from_name() {
    // boost::uuids::name_generator_sha1 in the namespace 8b8b58e2-3d21-4a24-9dcf-42e0f14001a2
    // (values dumped from the C++ implementation).
    for (name, expected) in [
        ("", "a01baf89-6722-503b-945e-7ed6fd8c7ebf"),
        ("Device:R", "c91813c0-395d-52b5-81e7-6f3fc5470216"),
        ("R_0402_1005Metric", "59849067-490b-54fa-a29e-e78b0a2224b2"),
        ("some known string", "6168d9fc-5f85-53f2-963c-cb948e229104"),
        (
            "a longer name that spans more than one sha1 block: \
             0123456789012345678901234567890123456789012345678901234567890123456789",
            "86ed6e6a-bd40-5fb8-bf31-a867d2e08386",
        ),
    ] {
        assert_eq!(Kiid::from_name(name).as_string(), expected, "{name:?}");
    }
}

#[test]
fn ordering_and_hash() {
    // boost::uuids::uuid compares its bytes lexicographically.
    let a = Kiid::parse("00000000-0000-0000-0000-0000000000ff").unwrap();
    let b = Kiid::parse("00000000-0000-0000-0000-000000000100").unwrap();
    let c = Kiid::parse("ff000000-0000-0000-0000-000000000000").unwrap();
    let d = Kiid::parse("00000000-0000-0000-8000-000000000000").unwrap();

    assert!(a < b);
    assert!(b < c);
    assert!(d < c);
    assert!(b < d);
    assert!(c > d);
    assert!(NILUUID < a);
    assert_eq!(Kiid::default(), NILUUID);

    assert_eq!(a.hash_value(), 17562157925649023534);
    assert_eq!(c.hash_value(), 3996712364485557191);

    let mut sorted = vec![c, d, b, a, NILUUID];
    sorted.sort();
    assert_eq!(sorted, vec![NILUUID, a, b, d, c]);
}

#[test]
fn increment() {
    let mut id = Kiid::parse("00000000-0000-0000-0000-0000000000ff").unwrap();
    id.increment();
    assert_eq!(id.as_string(), "00000000-0000-0000-0000-000000000100");

    let mut id = Kiid::parse("00000000-0000-0000-00ff-ffffffffffff").unwrap();
    id.increment();
    assert_eq!(id.as_string(), "00000000-0000-0000-0100-000000000000");

    let mut id = Kiid::parse("ffffffff-ffff-ffff-ffff-ffffffffffff").unwrap();
    id.increment();
    assert_eq!(id, NILUUID);
}

#[test]
fn from_deterministic_string_non_ascii() {
    // Hashing is over code points (the 32 bit wxChar platforms).
    assert_eq!(
        Kiid::from_deterministic_string("").as_string(),
        "cbf29ce4-8422-2325-9ce4-84222325cbf2"
    );
    assert_ne!(
        Kiid::from_deterministic_string("R\u{e9}sistance"),
        Kiid::from_deterministic_string("Resistance")
    );
}

#[test]
fn kiid_path() {
    let a = Kiid::parse("00000000-0000-0000-0000-00000000000a").unwrap();
    let b = Kiid::parse("00000000-0000-0000-0000-00000000000b").unwrap();
    let c = Kiid::parse("00000000-0000-0000-0000-00000000000c").unwrap();

    let text = format!("/{a}/{b}/{c}");
    let path = KiidPath::parse(&text).unwrap();

    assert_eq!(path.len(), 3);
    assert_eq!(path.0, vec![a, b, c]);
    assert_eq!(path.as_string(), text);
    assert_eq!(path.to_string(), text);

    // Empty steps are skipped; legacy timestamps are valid steps.
    assert_eq!(
        KiidPath::parse(&format!("{a}//{b}/")).unwrap().0,
        vec![a, b]
    );
    assert_eq!(
        KiidPath::parse("/5F0B8A2C").unwrap().0,
        vec![Kiid::from_legacy_timestamp(0x5F0B8A2C)]
    );
    assert_eq!(KiidPath::parse("").unwrap(), KiidPath::new());
    assert_eq!(KiidPath::parse("/").unwrap(), KiidPath::new());
    assert_eq!(KiidPath::new().as_string(), "");

    // Invalid steps: strict parsing fails, the C++ constructor substitutes random ids.
    assert_eq!(KiidPath::parse("/nonsense"), None);
    let mut generator = KiidGenerator::seeded(0);
    let random = KiidPath::from_string_or_random(&format!("/{a}/nonsense"), &mut generator);
    assert_eq!(random.len(), 2);
    assert_eq!(random[0], a);
    assert_eq!(
        random[1].as_string(),
        "ac0a7f8c-2faa-4497-b5a6-16b7c0cc21d8"
    );

    // MakeRelativeTo
    let parent: KiidPath = vec![a, b].into();
    let mut relative = path.clone();
    assert!(relative.make_relative_to(&parent));
    assert_eq!(relative.0, vec![c]);

    let other: KiidPath = vec![b].into();
    let mut unchanged = path.clone();
    assert!(!unchanged.make_relative_to(&other));
    assert_eq!(unchanged, path);

    // A longer path empties this one (as in C++).
    let mut short: KiidPath = vec![a].into();
    assert!(!short.make_relative_to(&path));
    assert!(short.is_empty());

    // IsContainedWithin / EndsWith
    assert!(path.is_contained_within(&parent));
    assert!(path.is_contained_within(&KiidPath::new()));
    assert!(!parent.is_contained_within(&path));
    assert!(!path.is_contained_within(&other));
    assert!(path.ends_with(&vec![b, c].into()));
    assert!(path.ends_with(&KiidPath::new()));
    assert!(!path.ends_with(&parent));

    // operator+ / operator+=
    let mut joined = parent.clone();
    joined += &vec![c].into();
    assert_eq!(joined, path);
    assert_eq!(parent.clone() + &vec![c].into(), path);

    // Ordering: shorter paths first, then element-wise.
    let pa: KiidPath = vec![c].into();
    let pb: KiidPath = vec![a, a].into();
    let pc: KiidPath = vec![a, b].into();
    assert!(pa < pb);
    assert!(pb < pc);
    assert!(pc > pa);
    assert!(pb >= pb.clone());

    // std::hash<KIID_PATH>
    assert_eq!(KiidPath::new().hash_value(), 0);
    let single: KiidPath = vec![a].into();
    assert_eq!(single.hash_value(), a.hash_value().wrapping_add(0x9e3779b9));
    assert_ne!(pb.hash_value(), pc.hash_value());

    let collected: KiidPath = path.iter().copied().collect();
    assert_eq!(collected, path);
    assert_eq!((&path).into_iter().count(), 3);
    assert_eq!(path.into_iter().last(), Some(c));
}

/// Port addition (`KiidGenerator::reserve_all`, review INT-2b): a reserved id is skipped, the
/// rest of the sequence is the C++ one for the seed. The ids are the first four of
/// `KIID::SeedGenerator( 5489 )` (see `seeded_sequences_match_cpp`).
#[test]
fn reserved_ids_are_skipped() {
    let first = [
        "5cbb91d0-f69e-4e22-aefa-e1e7791fc3d5",
        "2c358220-dfb7-47f8-8500-d3e9e1af9538",
        "ba4be2a1-2b09-444e-a368-f81825a6168c",
        "c4a84b47-1acd-4930-9f6d-008c10782dfe",
    ];
    let id = |s: &str| Kiid::parse(s).unwrap();

    let mut plain = KiidGenerator::default();
    assert_eq!(plain.generate().as_string(), first[0]);
    assert!(!plain.is_reserved(&id(first[1])));

    let mut generator = KiidGenerator::default();
    generator.reserve_all([id(first[0]), id(first[2])]);
    generator.reserve_all(std::iter::empty());
    assert!(generator.is_reserved(&id(first[0])));

    // A clone shares the reservations; reseeding keeps them
    let mut clone = generator.clone();
    assert_eq!(generator.generate().as_string(), first[1]);
    assert_eq!(generator.generate().as_string(), first[3]);
    clone.seed_generator(5489);
    assert_eq!(clone.random_uuid().as_string(), first[1]);
    assert_eq!(
        Kiid::from_string_or_random("not a uuid", &mut clone).as_string(),
        first[3]
    );
}
