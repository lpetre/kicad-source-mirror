/*
 * This program source code file is part of KiCad, a free EDA CAD application.
 *
 * Copyright The KiCad Developers, see AUTHORS.txt for contributors.
 *
 * This program is free software: you can redistribute it and/or modify it
 * under the terms of the GNU General Public License as published by the
 * Free Software Foundation, either version 3 of the License, or (at your
 * option) any later version.
 *
 * This program is distributed in the hope that it will be useful, but
 * WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
 * General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License along
 * with this program.  If not, see <http://www.gnu.org/licenses/>.
 */

/*
 * Differential oracle for kicad-zint: links the libzint.a of the reference build (zint 2.15.0 as
 * bundled in thirdparty/zint, compiled -O3 -DNDEBUG) and encodes generated cases exactly the way
 * PCB_BARCODE::ComputeBarcode does (UNICODE_MODE, show_hrt 0, option_1 = ECC for QR / Micro QR,
 * eci 26 for non-ASCII QR / Data Matrix text), then ZBarcode_Buffer_Vector( symbol, 0 ).
 *
 * The case generator is mirrored statement by statement in tests/zint_oracle.rs.
 *
 * Usage:
 *   zint_oracle hashes          one line "B <block> <fnv1a64>" per block of 1000 cases, "N <count>"
 *   zint_oracle case <n>        the result line of case n
 *   zint_oracle lines <a> <b>   the result lines of cases a .. b-1
 *   zint_oracle input <sym> <option_1> <eci> <hex bytes>   the result line of one input
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "zint.h"

/* ---- deterministic generator (mirrored in Rust) ---------------------------------------------- */

static uint64_t rng_state;

static uint64_t rng_next(void) {
    uint64_t z;
    rng_state += 0x9E3779B97F4A7C15ULL;
    z = rng_state;
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
    return z ^ (z >> 31);
}

static uint32_t rnd(uint32_t n) {
    return (uint32_t) (rng_next() % n);
}

static const char QR_ALNUM[] = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";
static const char C39_SET[] = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%abcxyz";
static const uint32_t SJIS_SPECIALS[] = {
    0x5C, 0x7E, 0xA5, 0x203E, 0xFF61, 0xFF9F, 0x3000, 0x3001, 0x30A2, 0x30F3, 0x4E00, 0x9FA0,
    0xE000, 0xE757, 0xFF5E, 0x2015, 0x00A7, 0x00B6, 0x0391, 0x0410,
};

#define NUM_CLASSES 12

/* One code point of class `cls` */
static uint32_t gen_cp(int cls) {
    uint32_t u;
    switch (cls) {
        case 0: return '0' + rnd(10);
        case 1: return (unsigned char) QR_ALNUM[rnd(45)];
        case 2: return (unsigned char) C39_SET[rnd(49)];
        case 3: return 0x20 + rnd(95);
        case 4: return rnd(128);
        case 5: return 0x80 + rnd(128);
        case 6: return 0x100 + rnd(0x700);
        case 7:
            u = 0x800 + rnd(0xF800);
            if (u >= 0xD800 && u <= 0xDFFF) u -= 0x800;
            return u;
        case 8: return 0x4E00 + rnd(0x51B0);
        case 9: return 0x10000 + rnd(0x100000);
        case 10: return SJIS_SPECIALS[rnd(sizeof(SJIS_SPECIALS) / sizeof(SJIS_SPECIALS[0]))];
        default: return rnd(10) < 8 ? '0' + rnd(10) : 'A' + rnd(26);
    }
}

static int put_utf8(uint32_t u, unsigned char *d) {
    if (u < 0x80) { d[0] = u; return 1; }
    if (u < 0x800) { d[0] = 0xC0 | (u >> 6); d[1] = 0x80 | (u & 0x3F); return 2; }
    if (u < 0x10000) {
        d[0] = 0xE0 | (u >> 12); d[1] = 0x80 | ((u >> 6) & 0x3F); d[2] = 0x80 | (u & 0x3F); return 3;
    }
    d[0] = 0xF0 | (u >> 18); d[1] = 0x80 | ((u >> 12) & 0x3F); d[2] = 0x80 | ((u >> 6) & 0x3F);
    d[3] = 0x80 | (u & 0x3F);
    return 4;
}

struct gcase {
    int symbology;
    int option_1;
    int eci_rule; /* 1: KiCad's rule (26 if QR / DM and non-ASCII), 0: no ECI */
    int len;      /* bytes */
    unsigned char data[32000];
};

static const int SYMS[5] = { BARCODE_CODE39, BARCODE_CODE128, BARCODE_QRCODE, BARCODE_MICROQR,
                             BARCODE_DATAMATRIX };

/* `n` characters: runs of classes from `classes` (count `ncls`) */
static void gen_text(struct gcase *c, int n, const int *classes, int ncls) {
    int i = 0;
    c->len = 0;
    while (i < n) {
        int cls = classes[rnd(ncls)];
        int run = 1 + rnd(ncls == 1 ? 1000000 : 24);
        while (run-- && i < n) {
            c->len += put_utf8(gen_cp(cls), c->data + c->len);
            i++;
        }
    }
}

static int max_chars(int sym) {
    switch (sym) {
        case BARCODE_CODE39: return 92;
        case BARCODE_CODE128: return 262;
        case BARCODE_MICROQR: return 40;
        case BARCODE_QRCODE: return 7200;
        default: return 3200;
    }
}

/* Sweep: sym x ecc x class x length */
static const int SWEEP_CLASSES[6] = { 0, 1, 3, 4, 5, 8 };

static int sweep_option1(int sym, int k) { /* k-th ECC choice, -1 when done */
    if (sym == BARCODE_QRCODE || sym == BARCODE_MICROQR) return k < 4 ? k + 1 : -2;
    return k == 0 ? -1 : -2;
}

static int sweep_step(int sym, int len) {
    if (sym == BARCODE_QRCODE) return len < 400 ? 1 : 41;
    if (sym == BARCODE_DATAMATRIX) return len < 400 ? 1 : 29;
    return 1;
}

static int sweep_max(int sym, int cls) {
    int m = max_chars(sym);
    if (sym == BARCODE_QRCODE) {
        if (cls == 1) m = 4400;
        else if (cls != 0) m = 3000;
        if (cls == 8) m = 1900;
    } else if (sym == BARCODE_DATAMATRIX) {
        if (cls != 0) m = 1600;
    }
    return m;
}

/* Copies of zint's qr_data_codewords (qr.h) and dm_matrixbytes (dmatrix.h), for boundary cases */
static const int QR_CW[160] = { 19, 34, 55, 80, 108, 136, 156, 194, 232, 274, 324, 370, 428, 461, 523, 589, 647, 721, 795, 861, 932, 1006, 1094, 1174, 1276, 1370, 1468, 1531, 1631, 1735, 1843, 1955, 2071, 2191, 2306, 2434, 2566, 2702, 2812, 2956 , 16, 28, 44, 64, 86, 108, 124, 154, 182, 216, 254, 290, 334, 365, 415, 453, 507, 563, 627, 669, 714, 782, 860, 914, 1000, 1062, 1128, 1193, 1267, 1373, 1455, 1541, 1631, 1725, 1812, 1914, 1992, 2102, 2216, 2334 , 13, 22, 34, 48, 62, 76, 88, 110, 132, 154, 180, 206, 244, 261, 295, 325, 367, 397, 445, 485, 512, 568, 614, 664, 718, 754, 808, 871, 911, 985, 1033, 1115, 1171, 1231, 1286, 1354, 1426, 1502, 1582, 1666 , 9, 16, 26, 36, 46, 60, 66, 86, 100, 122, 140, 158, 180, 197, 223, 253, 283, 313, 341, 385, 406, 442, 464, 514, 538, 596, 628, 661, 701, 745, 793, 845, 901, 961, 986, 1054, 1096, 1142, 1222, 1276 };
static const int DM_BYTES[48] = { 3,5,5,8,10,12,16,18,18,22,22,24,30,32,32,36,38,43,44,44,49,49,56,62,62,63,64,70,72,80,84,86,90,108,114,118,144,174,204,280,368,456,576,696,816,1050,1304,1558 };

/* Characters of class `cls` (0 digits, 1 alphanumeric, else bytes) filling QR version `v` (1-40) at ECC `e` */
static int qr_capacity(int e, int v, int cls) {
    int cci, bits, r;
    if (cls == 0) cci = v < 10 ? 10 : v < 27 ? 12 : 14;
    else if (cls == 1) cci = v < 10 ? 9 : v < 27 ? 11 : 13;
    else cci = v < 10 ? 8 : 16;
    bits = 8 * QR_CW[e * 40 + v - 1] - 4 - cci;
    if (cls == 0) {
        r = bits % 10;
        return (bits / 10) * 3 + (r >= 7 ? 2 : r >= 4 ? 1 : 0);
    }
    if (cls == 1) return (bits / 11) * 2 + (bits % 11 >= 6);
    return bits / 8;
}

struct sweep_entry {
    int symbology, option_1, cls, len;
};
static struct sweep_entry *sweep;
static long sweep_n;

/* Builds the sweep list: sym x ecc x class x length, in this nesting order */
static void sweep_init(void) {
    int pass, s, k, ci, len;
    for (pass = 0; pass < 2; pass++) {
        sweep_n = 0;
        for (s = 0; s < 5; s++)
            for (k = 0; sweep_option1(SYMS[s], k) != -2; k++)
                for (ci = 0; ci < 6; ci++)
                    for (len = 1; len <= sweep_max(SYMS[s], SWEEP_CLASSES[ci]);
                            len += sweep_step(SYMS[s], len)) {
                        if (pass) {
                            sweep[sweep_n].symbology = SYMS[s];
                            sweep[sweep_n].option_1 = sweep_option1(SYMS[s], k);
                            sweep[sweep_n].cls = SWEEP_CLASSES[ci];
                            sweep[sweep_n].len = len;
                        }
                        sweep_n++;
                    }
        /* Boundary cases: QR version capacities and Data Matrix size capacities, -1 / 0 / +1 */
        {
            static const int qr_cls[3] = { 0, 1, 3 };
            int e, v, d, kk, len2;
            for (e = 0; e < 4; e++)
                for (v = 1; v <= 40; v++)
                    for (kk = 0; kk < 3; kk++)
                        for (d = -1; d <= 1; d++) {
                            len2 = qr_capacity(e, v, qr_cls[kk]) + d;
                            if (pass) {
                                sweep[sweep_n].symbology = BARCODE_QRCODE;
                                sweep[sweep_n].option_1 = e + 1;
                                sweep[sweep_n].cls = qr_cls[kk];
                                sweep[sweep_n].len = len2;
                            }
                            sweep_n++;
                        }
            for (kk = 0; kk < 48; kk++)
                for (d = -1; d <= 1; d++)
                    for (e = 0; e < 2; e++) {
                        len2 = (e == 0 ? 2 * DM_BYTES[kk] : (3 * DM_BYTES[kk]) / 2) + d;
                        if (pass) {
                            sweep[sweep_n].symbology = BARCODE_DATAMATRIX;
                            sweep[sweep_n].option_1 = -1;
                            sweep[sweep_n].cls = e;
                            sweep[sweep_n].len = len2;
                        }
                        sweep_n++;
                    }
        }
        if (!pass) sweep = (struct sweep_entry *) calloc(sweep_n, sizeof(*sweep));
    }
}

#define RANDOM_CASES 300000L

static long total_cases(void) {
    return sweep_n + RANDOM_CASES;
}

/* Builds case `idx` */
static void gen_case(long idx, struct gcase *c) {
    if (idx < sweep_n) {
        const struct sweep_entry *e = &sweep[idx];
        rng_state = 0x5EED0000ULL + (uint64_t) idx;
        c->symbology = e->symbology;
        c->option_1 = e->option_1;
        c->eci_rule = 1;
        gen_text(c, e->len, &e->cls, 1);
        return;
    }
    {
        int classes[3];
        int ncls, i, nchars, r;
        rng_state = 0xC0FFEE0000000000ULL + (uint64_t) (idx - sweep_n);
        c->symbology = SYMS[rnd(5)];
        if (c->symbology == BARCODE_QRCODE) c->option_1 = 1 + rnd(4);
        else if (c->symbology == BARCODE_MICROQR) c->option_1 = rnd(10) == 0 ? 4 : 1 + rnd(3);
        else c->option_1 = -1;
        c->eci_rule = rnd(10) != 0;
        ncls = 1 + rnd(3);
        for (i = 0; i < ncls; i++) classes[i] = rnd(NUM_CLASSES);
        r = rnd(100);
        if (c->symbology == BARCODE_QRCODE || c->symbology == BARCODE_DATAMATRIX) {
            nchars = r < 70 ? 1 + rnd(100) : r < 97 ? 1 + rnd(1000) : 1 + rnd(max_chars(c->symbology));
        } else {
            nchars = 1 + rnd(max_chars(c->symbology));
        }
        gen_text(c, nchars, classes, ncls);
    }
}

/* ---- encoding + result line ------------------------------------------------------------------ */

static int is_ascii(const unsigned char *d, int len) {
    int i;
    for (i = 0; i < len; i++) if (d[i] & 0x80) return 0;
    return 1;
}

static char line[8 * 1024 * 1024];
static size_t lp;

static void out(const char *fmt, ...) __attribute__((format(printf, 1, 2)));
#include <stdarg.h>
static void out(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    lp += vsnprintf(line + lp, sizeof(line) - lp, fmt, ap);
    va_end(ap);
}

static uint32_t fbits(float f) {
    uint32_t u;
    memcpy(&u, &f, 4);
    return u;
}

/* Result line of one input. Format (fields separated by '|'):
   ret|errtxt  -- and if ret < ZINT_ERROR:
   |rows,width,height|o1,o2,o3|row heights|bitmap rows|vret|verrtxt|vw,vh|rects|hexagons */
static void encode_line(int sym, int option_1, int eci, const unsigned char *data, int len) {
    struct zint_symbol *symbol = ZBarcode_Create();
    int ret, r, c;
    struct zint_vector_rect *rect;
    struct zint_vector_hexagon *hex;

    lp = 0;
    symbol->input_mode = UNICODE_MODE;
    symbol->show_hrt = 0;
    symbol->symbology = sym;
    if (sym == BARCODE_QRCODE || sym == BARCODE_MICROQR) symbol->option_1 = option_1;
    symbol->eci = eci;

    ret = ZBarcode_Encode(symbol, data, len);
    if (eci == 0 && symbol->eci != 0) {
        /* `get_best_eci_segs()` retry ran: not ported */
        out("NP");
        ZBarcode_Delete(symbol);
        return;
    }
    out("%d|%s", ret, symbol->errtxt);
    if (ret < ZINT_ERROR) {
        out("|%d,%d,%08x|%d,%d,%d|", symbol->rows, symbol->width, fbits(symbol->height),
            symbol->option_1, symbol->option_2, symbol->option_3);
        for (r = 0; r < symbol->rows; r++) out("%s%08x", r ? "," : "", fbits(symbol->row_height[r]));
        out("|");
        for (r = 0; r < symbol->rows; r++) {
            if (r) out(",");
            for (c = 0; c < symbol->width; c += 4) {
                int nib = 0, b;
                for (b = 0; b < 4 && c + b < symbol->width; b++)
                    if ((symbol->encoded_data[r][(c + b) >> 3] >> ((c + b) & 7)) & 1) nib |= 8 >> b;
                out("%x", nib);
            }
        }
        ret = ZBarcode_Buffer_Vector(symbol, 0);
        out("|%d|%s", ret, symbol->errtxt);
        if (ret < ZINT_ERROR) {
            out("|%08x,%08x|", fbits(symbol->vector->width), fbits(symbol->vector->height));
            for (rect = symbol->vector->rectangles; rect; rect = rect->next)
                out("%s%08x,%08x,%08x,%08x", rect == symbol->vector->rectangles ? "" : ";", fbits(rect->x),
                    fbits(rect->y), fbits(rect->width), fbits(rect->height));
            out("|");
            for (hex = symbol->vector->hexagons; hex; hex = hex->next)
                out("%08x,%08x,%08x,%d;", fbits(hex->x), fbits(hex->y), fbits(hex->diameter), hex->rotation);
        }
    }
    ZBarcode_Delete(symbol);
}

static void case_line(long idx) {
    static struct gcase c;
    int eci;
    gen_case(idx, &c);
    eci = c.eci_rule && (c.symbology == BARCODE_QRCODE || c.symbology == BARCODE_DATAMATRIX)
            && !is_ascii(c.data, c.len) ? 26 : 0;
    encode_line(c.symbology, c.option_1, eci, c.data, c.len);
}

int main(int argc, char **argv) {
    sweep_init();
    if (argc >= 2 && strcmp(argv[1], "hashes") == 0) {
        long n = total_cases(), i;
        uint64_t h = 0xcbf29ce484222325ULL;
        printf("N %ld\n", n);
        for (i = 0; i < n; i++) {
            size_t k;
            case_line(i);
            line[lp++] = '\n';
            for (k = 0; k < lp; k++) {
                h ^= (unsigned char) line[k];
                h *= 0x100000001b3ULL;
            }
            if (i % 1000 == 999 || i == n - 1) {
                printf("B %ld %016llx\n", i / 1000, (unsigned long long) h);
                h = 0xcbf29ce484222325ULL;
            }
        }
        return 0;
    }
    if (argc == 4 && strcmp(argv[1], "lines") == 0) {
        long i, to = atol(argv[3]);
        for (i = atol(argv[2]); i < to && i < total_cases(); i++) {
            case_line(i);
            fwrite(line, 1, lp, stdout);
            fputc('\n', stdout);
        }
        return 0;
    }
    if (argc == 3 && strcmp(argv[1], "case") == 0) {
        case_line(atol(argv[2]));
        fwrite(line, 1, lp, stdout);
        fputc('\n', stdout);
        return 0;
    }
    if (argc == 6 && strcmp(argv[1], "input") == 0) {
        static unsigned char data[40000];
        const char *h = argv[5];
        int len = 0;
        while (h[0] && h[1]) {
            unsigned int v;
            sscanf(h, "%2x", &v);
            data[len++] = (unsigned char) v;
            h += 2;
        }
        encode_line(atoi(argv[2]), atoi(argv[3]), atoi(argv[4]), data, len);
        fwrite(line, 1, lp, stdout);
        fputc('\n', stdout);
        return 0;
    }
    fprintf(stderr, "usage: zint_oracle hashes | case <n> | input <sym> <option_1> <eci> <hex>\n");
    return 2;
}
