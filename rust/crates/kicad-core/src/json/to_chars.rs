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
//
// Ported from the bundled nlohmann/json 3.11.3 (`thirdparty/nlohmann_json/nlohmann/json.hpp`):
// SPDX-FileCopyrightText: 2013-2023 Niels Lohmann <https://nlohmann.me>
// SPDX-FileCopyrightText: 2009 Florian Loitsch <https://florian.loitsch.com/>
// SPDX-License-Identifier: MIT

//! Port of `nlohmann::detail::to_chars` (Grisu2, `detail/conversions/to_chars.hpp`) and
//! `dtoa_impl::format_buffer` from the bundled nlohmann/json 3.11.3.
//!
//! The algorithm is integer-only, so it gives the same digits on every target.  Grisu2 is not
//! always the shortest representation (it prints `9.999999999999999e+22` for `1e23`), which is
//! why `serde_json`/`ryu` output cannot be used for `.kicad_pro` files.

#[derive(Clone, Copy)]
struct DiyFp {
    f: u64,
    e: i32,
}

impl DiyFp {
    fn sub(x: DiyFp, y: DiyFp) -> DiyFp {
        debug_assert!(x.e == y.e);
        debug_assert!(x.f >= y.f);
        DiyFp { f: x.f - y.f, e: x.e }
    }

    fn mul(x: DiyFp, y: DiyFp) -> DiyFp {
        let u_lo = x.f & 0xFFFF_FFFF;
        let u_hi = x.f >> 32;
        let v_lo = y.f & 0xFFFF_FFFF;
        let v_hi = y.f >> 32;

        let p0 = u_lo * v_lo;
        let p1 = u_lo * v_hi;
        let p2 = u_hi * v_lo;
        let p3 = u_hi * v_hi;

        let p0_hi = p0 >> 32;
        let p1_lo = p1 & 0xFFFF_FFFF;
        let p1_hi = p1 >> 32;
        let p2_lo = p2 & 0xFFFF_FFFF;
        let p2_hi = p2 >> 32;

        let mut q = p0_hi + p1_lo + p2_lo;
        q += 1u64 << (64 - 32 - 1); // round, ties up

        let h = p3 + p2_hi + p1_hi + (q >> 32);
        DiyFp { f: h, e: x.e + y.e + 64 }
    }

    fn normalize(mut x: DiyFp) -> DiyFp {
        debug_assert!(x.f != 0);
        while (x.f >> 63) == 0 {
            x.f <<= 1;
            x.e -= 1;
        }
        x
    }

    fn normalize_to(x: DiyFp, target_exponent: i32) -> DiyFp {
        let delta = x.e - target_exponent;
        debug_assert!(delta >= 0);
        debug_assert!(((x.f << delta) >> delta) == x.f);
        DiyFp { f: x.f << delta, e: target_exponent }
    }
}

struct Boundaries {
    w: DiyFp,
    minus: DiyFp,
    plus: DiyFp,
}

fn compute_boundaries(value: f64) -> Boundaries {
    const K_PRECISION: i32 = 53;
    const K_BIAS: i32 = 1024 - 1 + (K_PRECISION - 1);
    const K_MIN_EXP: i32 = 1 - K_BIAS;
    const K_HIDDEN_BIT: u64 = 1u64 << (K_PRECISION - 1);

    let bits = value.to_bits();
    let e = bits >> (K_PRECISION - 1);
    let f = bits & (K_HIDDEN_BIT - 1);

    let is_denormal = e == 0;
    let v = if is_denormal {
        DiyFp { f, e: K_MIN_EXP }
    } else {
        DiyFp { f: f + K_HIDDEN_BIT, e: e as i32 - K_BIAS }
    };

    let lower_boundary_is_closer = f == 0 && e > 1;
    let m_plus = DiyFp { f: 2 * v.f + 1, e: v.e - 1 };
    let m_minus = if lower_boundary_is_closer {
        DiyFp { f: 4 * v.f - 1, e: v.e - 2 }
    } else {
        DiyFp { f: 2 * v.f - 1, e: v.e - 1 }
    };

    let w_plus = DiyFp::normalize(m_plus);
    let w_minus = DiyFp::normalize_to(m_minus, w_plus.e);

    Boundaries { w: DiyFp::normalize(v), minus: w_minus, plus: w_plus }
}

const K_ALPHA: i32 = -60;
const K_GAMMA: i32 = -32;

struct CachedPower {
    f: u64,
    e: i32,
    k: i32,
}

const fn cp(f: u64, e: i32, k: i32) -> CachedPower {
    CachedPower { f, e, k }
}

static K_CACHED_POWERS: [CachedPower; 79] = [
    cp(0xAB70FE17C79AC6CA, -1060, -300),
    cp(0xFF77B1FCBEBCDC4F, -1034, -292),
    cp(0xBE5691EF416BD60C, -1007, -284),
    cp(0x8DD01FAD907FFC3C, -980, -276),
    cp(0xD3515C2831559A83, -954, -268),
    cp(0x9D71AC8FADA6C9B5, -927, -260),
    cp(0xEA9C227723EE8BCB, -901, -252),
    cp(0xAECC49914078536D, -874, -244),
    cp(0x823C12795DB6CE57, -847, -236),
    cp(0xC21094364DFB5637, -821, -228),
    cp(0x9096EA6F3848984F, -794, -220),
    cp(0xD77485CB25823AC7, -768, -212),
    cp(0xA086CFCD97BF97F4, -741, -204),
    cp(0xEF340A98172AACE5, -715, -196),
    cp(0xB23867FB2A35B28E, -688, -188),
    cp(0x84C8D4DFD2C63F3B, -661, -180),
    cp(0xC5DD44271AD3CDBA, -635, -172),
    cp(0x936B9FCEBB25C996, -608, -164),
    cp(0xDBAC6C247D62A584, -582, -156),
    cp(0xA3AB66580D5FDAF6, -555, -148),
    cp(0xF3E2F893DEC3F126, -529, -140),
    cp(0xB5B5ADA8AAFF80B8, -502, -132),
    cp(0x87625F056C7C4A8B, -475, -124),
    cp(0xC9BCFF6034C13053, -449, -116),
    cp(0x964E858C91BA2655, -422, -108),
    cp(0xDFF9772470297EBD, -396, -100),
    cp(0xA6DFBD9FB8E5B88F, -369, -92),
    cp(0xF8A95FCF88747D94, -343, -84),
    cp(0xB94470938FA89BCF, -316, -76),
    cp(0x8A08F0F8BF0F156B, -289, -68),
    cp(0xCDB02555653131B6, -263, -60),
    cp(0x993FE2C6D07B7FAC, -236, -52),
    cp(0xE45C10C42A2B3B06, -210, -44),
    cp(0xAA242499697392D3, -183, -36),
    cp(0xFD87B5F28300CA0E, -157, -28),
    cp(0xBCE5086492111AEB, -130, -20),
    cp(0x8CBCCC096F5088CC, -103, -12),
    cp(0xD1B71758E219652C, -77, -4),
    cp(0x9C40000000000000, -50, 4),
    cp(0xE8D4A51000000000, -24, 12),
    cp(0xAD78EBC5AC620000, 3, 20),
    cp(0x813F3978F8940984, 30, 28),
    cp(0xC097CE7BC90715B3, 56, 36),
    cp(0x8F7E32CE7BEA5C70, 83, 44),
    cp(0xD5D238A4ABE98068, 109, 52),
    cp(0x9F4F2726179A2245, 136, 60),
    cp(0xED63A231D4C4FB27, 162, 68),
    cp(0xB0DE65388CC8ADA8, 189, 76),
    cp(0x83C7088E1AAB65DB, 216, 84),
    cp(0xC45D1DF942711D9A, 242, 92),
    cp(0x924D692CA61BE758, 269, 100),
    cp(0xDA01EE641A708DEA, 295, 108),
    cp(0xA26DA3999AEF774A, 322, 116),
    cp(0xF209787BB47D6B85, 348, 124),
    cp(0xB454E4A179DD1877, 375, 132),
    cp(0x865B86925B9BC5C2, 402, 140),
    cp(0xC83553C5C8965D3D, 428, 148),
    cp(0x952AB45CFA97A0B3, 455, 156),
    cp(0xDE469FBD99A05FE3, 481, 164),
    cp(0xA59BC234DB398C25, 508, 172),
    cp(0xF6C69A72A3989F5C, 534, 180),
    cp(0xB7DCBF5354E9BECE, 561, 188),
    cp(0x88FCF317F22241E2, 588, 196),
    cp(0xCC20CE9BD35C78A5, 614, 204),
    cp(0x98165AF37B2153DF, 641, 212),
    cp(0xE2A0B5DC971F303A, 667, 220),
    cp(0xA8D9D1535CE3B396, 694, 228),
    cp(0xFB9B7CD9A4A7443C, 720, 236),
    cp(0xBB764C4CA7A44410, 747, 244),
    cp(0x8BAB8EEFB6409C1A, 774, 252),
    cp(0xD01FEF10A657842C, 800, 260),
    cp(0x9B10A4E5E9913129, 827, 268),
    cp(0xE7109BFBA19C0C9D, 853, 276),
    cp(0xAC2820D9623BF429, 880, 284),
    cp(0x80444B5E7AA7CF85, 907, 292),
    cp(0xBF21E44003ACDD2D, 933, 300),
    cp(0x8E679C2F5E44FF8F, 960, 308),
    cp(0xD433179D9C8CB841, 986, 316),
    cp(0x9E19DB92B4E31BA9, 1013, 324),
];

fn get_cached_power_for_binary_exponent(e: i32) -> &'static CachedPower {
    const K_CACHED_POWERS_MIN_DEC_EXP: i32 = -300;
    const K_CACHED_POWERS_DEC_STEP: i32 = 8;

    debug_assert!(e >= -1500);
    debug_assert!(e <= 1500);
    let f = K_ALPHA - e - 1;
    // C++ integer division truncates toward zero, as does Rust's `/` on i32.
    let k = (f * 78913) / (1 << 18) + i32::from(f > 0);

    let index = (-K_CACHED_POWERS_MIN_DEC_EXP + k + (K_CACHED_POWERS_DEC_STEP - 1)) / K_CACHED_POWERS_DEC_STEP;
    debug_assert!(index >= 0);
    let cached = &K_CACHED_POWERS[index as usize];
    debug_assert!(K_ALPHA <= cached.e + e + 64);
    debug_assert!(K_GAMMA >= cached.e + e + 64);
    cached
}

fn find_largest_pow10(n: u32) -> (i32, u32) {
    if n >= 1_000_000_000 {
        (10, 1_000_000_000)
    } else if n >= 100_000_000 {
        (9, 100_000_000)
    } else if n >= 10_000_000 {
        (8, 10_000_000)
    } else if n >= 1_000_000 {
        (7, 1_000_000)
    } else if n >= 100_000 {
        (6, 100_000)
    } else if n >= 10_000 {
        (5, 10_000)
    } else if n >= 1_000 {
        (4, 1_000)
    } else if n >= 100 {
        (3, 100)
    } else if n >= 10 {
        (2, 10)
    } else {
        (1, 1)
    }
}

fn grisu2_round(buf: &mut [u8], len: usize, dist: u64, delta: u64, mut rest: u64, ten_k: u64) {
    debug_assert!(len >= 1);
    debug_assert!(dist <= delta);
    debug_assert!(rest <= delta);
    debug_assert!(ten_k > 0);

    while rest < dist && delta - rest >= ten_k && (rest + ten_k < dist || dist - rest > rest + ten_k - dist) {
        debug_assert!(buf[len - 1] != b'0');
        buf[len - 1] -= 1;
        rest += ten_k;
    }
}

fn grisu2_digit_gen(
    buffer: &mut [u8],
    length: &mut usize,
    decimal_exponent: &mut i32,
    m_minus: DiyFp,
    w: DiyFp,
    m_plus: DiyFp,
) {
    debug_assert!(m_plus.e >= K_ALPHA);
    debug_assert!(m_plus.e <= K_GAMMA);

    let mut delta = DiyFp::sub(m_plus, m_minus).f;
    let mut dist = DiyFp::sub(m_plus, w).f;

    let one = DiyFp { f: 1u64 << (-m_plus.e), e: m_plus.e };

    let mut p1 = (m_plus.f >> (-one.e)) as u32;
    let mut p2 = m_plus.f & (one.f - 1);

    debug_assert!(p1 > 0);

    let (k, mut pow10) = find_largest_pow10(p1);
    let mut n = k;

    while n > 0 {
        let d = p1 / pow10;
        let r = p1 % pow10;
        debug_assert!(d <= 9);
        buffer[*length] = b'0' + d as u8;
        *length += 1;
        p1 = r;
        n -= 1;

        let rest = (u64::from(p1) << (-one.e)) + p2;

        if rest <= delta {
            *decimal_exponent += n;
            let ten_n = u64::from(pow10) << (-one.e);
            grisu2_round(buffer, *length, dist, delta, rest, ten_n);
            return;
        }

        pow10 /= 10;
    }

    debug_assert!(p2 > delta);

    let mut m = 0;

    loop {
        debug_assert!(p2 <= u64::MAX / 10);
        p2 *= 10;
        let d = p2 >> (-one.e);
        let r = p2 & (one.f - 1);
        debug_assert!(d <= 9);
        buffer[*length] = b'0' + d as u8;
        *length += 1;
        p2 = r;
        m += 1;

        delta *= 10;
        dist *= 10;

        if p2 <= delta {
            break;
        }
    }

    *decimal_exponent -= m;

    let ten_m = one.f;
    grisu2_round(buffer, *length, dist, delta, p2, ten_m);
}

/// Port of `dtoa_impl::grisu2( buf, len, decimal_exponent, value )`: returns the digit count and
/// the decimal exponent such that `value == digits * 10^decimal_exponent`.
fn grisu2(buf: &mut [u8], value: f64) -> (usize, i32) {
    debug_assert!(value.is_finite());
    debug_assert!(value > 0.0);

    let w = compute_boundaries(value);
    let (m_minus, v, m_plus) = (w.minus, w.w, w.plus);

    debug_assert!(m_plus.e == m_minus.e);
    debug_assert!(m_plus.e == v.e);

    let cached = get_cached_power_for_binary_exponent(m_plus.e);
    let c_minus_k = DiyFp { f: cached.f, e: cached.e };

    let w = DiyFp::mul(v, c_minus_k);
    let w_minus = DiyFp::mul(m_minus, c_minus_k);
    let w_plus = DiyFp::mul(m_plus, c_minus_k);

    let big_m_minus = DiyFp { f: w_minus.f + 1, e: w_minus.e };
    let big_m_plus = DiyFp { f: w_plus.f - 1, e: w_plus.e };

    let mut decimal_exponent = -cached.k;
    let mut len = 0usize;
    grisu2_digit_gen(buf, &mut len, &mut decimal_exponent, big_m_minus, w, big_m_plus);
    (len, decimal_exponent)
}

fn append_exponent(out: &mut Vec<u8>, mut e: i32) {
    debug_assert!(e > -1000);
    debug_assert!(e < 1000);

    if e < 0 {
        e = -e;
        out.push(b'-');
    } else {
        out.push(b'+');
    }

    let mut k = e as u32;

    if k < 10 {
        out.push(b'0');
        out.push(b'0' + k as u8);
    } else if k < 100 {
        out.push(b'0' + (k / 10) as u8);
        k %= 10;
        out.push(b'0' + k as u8);
    } else {
        out.push(b'0' + (k / 100) as u8);
        k %= 100;
        out.push(b'0' + (k / 10) as u8);
        k %= 10;
        out.push(b'0' + k as u8);
    }
}

/// Port of `dtoa_impl::format_buffer` (builds the text instead of shuffling bytes in place).
fn format_buffer(out: &mut Vec<u8>, digits: &[u8], decimal_exponent: i32, min_exp: i32, max_exp: i32) {
    debug_assert!(min_exp < 0);
    debug_assert!(max_exp > 0);

    let k = digits.len() as i32;
    let n = k + decimal_exponent;

    if k <= n && n <= max_exp {
        // digits[000].0
        out.extend_from_slice(digits);
        out.extend(std::iter::repeat_n(b'0', (n - k) as usize));
        out.extend_from_slice(b".0");
        return;
    }

    if 0 < n && n <= max_exp {
        // dig.its
        out.extend_from_slice(&digits[..n as usize]);
        out.push(b'.');
        out.extend_from_slice(&digits[n as usize..]);
        return;
    }

    if min_exp < n && n <= 0 {
        // 0.[000]digits
        out.extend_from_slice(b"0.");
        out.extend(std::iter::repeat_n(b'0', (-n) as usize));
        out.extend_from_slice(digits);
        return;
    }

    if k == 1 {
        // dE+123
        out.push(digits[0]);
    } else {
        // d.igitsE+123
        out.push(digits[0]);
        out.push(b'.');
        out.extend_from_slice(&digits[1..]);
    }

    out.push(b'e');
    append_exponent(out, n - 1);
}

/// Port of `nlohmann::detail::to_chars( first, last, double )`. `value` must be finite.
pub fn to_chars(out: &mut Vec<u8>, mut value: f64) {
    debug_assert!(value.is_finite());

    if value.is_sign_negative() {
        value = -value;
        out.push(b'-');
    }

    if value == 0.0 {
        out.extend_from_slice(b"0.0");
        return;
    }

    let mut buf = [0u8; 32];
    let (len, decimal_exponent) = grisu2(&mut buf, value);

    const K_MIN_EXP: i32 = -4;
    const K_MAX_EXP: i32 = 15; // std::numeric_limits<double>::digits10

    format_buffer(out, &buf[..len], decimal_exponent, K_MIN_EXP, K_MAX_EXP);
}

