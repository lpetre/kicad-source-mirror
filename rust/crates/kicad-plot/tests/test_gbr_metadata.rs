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

//! Unit tests of the Gerber metadata port (PLOT-02): check values from the analysis
//! (`rust/docs/analysis/plot-fab.md` 6.2, 6.3, 4.2.3) and the goldens of `qa/data/cli`. The
//! exhaustive comparison with the C++ is `test_plot_base_oracle.rs`. There is no C++ Boost
//! test of `gbr_metadata.cpp`.

use kicad_plot::PlotFormat;
use kicad_plot::common_plot_functions::get_default_plot_extension;
use kicad_plot::gbr_metadata::*;
use kicad_plot::gbr_netlist_metadata::{GbrNetlistMetadata, gbr_netinfo};

#[test]
fn project_guid_check_values() {
    // From qa/data/cli/basic_test/basic_test-PTH-drl.gbr and Issue24143-F_Cu.gbr
    assert_eq!(
        gbr_make_project_guid_from_string("basic_test.kicad_pcb"),
        "62617369-635f-4746-9573-742e6b696361"
    );
    assert_eq!(
        gbr_make_project_guid_from_string("Issue24143.kicad_pcb"),
        "49737375-6532-4343-9134-332e6b696361"
    );
}

#[test]
fn creation_date_formats() {
    let date = GbrCreationDate {
        year: 2018,
        month: 11,
        day: 21,
        hour: 8,
        minute: 49,
        second: 16,
        utc_offset_minutes: 60,
    };

    assert_eq!(
        gbr_make_creation_date_attribute_string(GbrNcStringFormat::X1, &date),
        "G04 #@! TF.CreationDate,2018-11-21T08:49:16+01:00*"
    );
    assert_eq!(
        gbr_make_creation_date_attribute_string(GbrNcStringFormat::X2, &date),
        "%TF.CreationDate,2018-11-21T08:49:16+01:00*%"
    );
    assert_eq!(
        gbr_make_creation_date_attribute_string(GbrNcStringFormat::Gbrjob, &date),
        "2018-11-21T08:49:16+01:00"
    );

    let utc_minus = GbrCreationDate {
        utc_offset_minutes: -330,
        ..date
    };
    assert_eq!(
        gbr_make_creation_date_attribute_string(GbrNcStringFormat::Ncdrill, &utc_minus),
        "; #@! TF.CreationDate,2018-11-21T08:49:16-05:30"
    );
}

#[test]
fn aperture_function_strings() {
    let f = |a, x1| GbrApertureMetadata::format_attribute_static(a, x1, "");
    assert_eq!(
        f(GbrApertureAttrib::Conductor, false),
        "%TA.AperFunction,Conductor*%\n"
    );
    assert_eq!(
        f(GbrApertureAttrib::SmdpadCudef, true),
        "G04 #@! TA.AperFunction,SMDPad,CuDef*\n"
    );
    assert_eq!(
        f(GbrApertureAttrib::CmpOblongDrill, false),
        "G04 aperture for slot hole*\n%TA.AperFunction,ComponentDrill*%\n"
    );
    assert_eq!(f(GbrApertureAttrib::None, false), "");
    assert_eq!(f(GbrApertureAttrib::End, true), "");

    let mut md = GbrMetadata::new();
    md.set_aperture_attrib_custom("NPTH,Drill");
    assert_eq!(
        md.aperture_metadata.format_attribute(false),
        "%TA.AperFunction,Other,NPTH,Drill*%\n"
    );
}

#[test]
fn escaping() {
    assert_eq!(
        format_string_to_gerber("a,b*c%d\\e"),
        "a\\u002Cb\\u002Ac\\u0025d\\u005Ce"
    );
    assert_eq!(format_string_to_gerber("\u{e9}"), "\\u00E9");
    // Code points above the BMP keep their low 16 bits only
    assert_eq!(format_string_to_gerber("\u{1F600}"), "\\uF600");
    // Quoted strings are not converted
    assert_eq!(format_string_to_gerber("\"a,b\""), "\"a,b\"");
    // std::string from a const char*: truncated at the first NUL
    assert_eq!(format_string_to_gerber("ab\0cd"), "ab");
    assert_eq!(
        convert_not_allowed_chars_in_gerber("\"\u{e9}", true, true),
        "\"\\u0022\u{e9}\""
    );
}

/// The clear-and-repeat case of `rust/docs/analysis/plot-fab.md` 4.2.3: when an attribute
/// disappears the dictionary is cleared (`%TD*%` by the caller) and the full set is printed.
#[test]
fn net_attribute_sequence() {
    let mut dict = String::new();

    let mut pad = GbrNetlistMetadata::new();
    pad.net_attrib_type = gbr_netinfo::ALL;
    pad.cmpref = "R5".into();
    pad.netname = "GND".into();
    pad.padname.set_field("3", false, false);

    let r = format_net_attribute(&mut dict, Some(&pad), false).unwrap();
    assert_eq!(
        r.printed_text.as_deref(),
        Some("%TO.P,R5,3*%\n%TO.N,GND*%\n")
    );
    assert!(!r.clear_previous_attributes);

    // Same attributes: nothing printed
    let r = format_net_attribute(&mut dict, Some(&pad), false).unwrap();
    assert_eq!(r.printed_text, None);

    // A track of the same net: TO.P disappears -> clear and print the full set
    let mut track = GbrNetlistMetadata::new();
    track.net_attrib_type = gbr_netinfo::NET;
    track.netname = "GND".into();
    let r = format_net_attribute(&mut dict, Some(&track), false).unwrap();
    assert!(r.clear_previous_attributes);
    assert_eq!(r.printed_text.as_deref(), Some("%TO.N,GND*%\n"));

    // Net name changes only: the short list
    track.netname = String::new();
    let r = format_net_attribute(&mut dict, Some(&track), true).unwrap();
    assert!(!r.clear_previous_attributes);
    assert_eq!(r.printed_text.as_deref(), Some("G04 #@! TO.N,N/C*\n"));

    track.not_in_net = true;
    let r = format_net_attribute(&mut dict, Some(&track), false).unwrap();
    assert_eq!(r.printed_text.as_deref(), Some("%TO.N,*%\n"));

    track.net_attrib_type = gbr_netinfo::UNSPECIFIED;
    assert_eq!(format_net_attribute(&mut dict, Some(&track), false), None);
    assert_eq!(format_net_attribute(&mut dict, None, false), None);
}

#[test]
fn default_plot_extensions() {
    assert_eq!(get_default_plot_extension(PlotFormat::Gerber), "gbr");
    assert_eq!(get_default_plot_extension(PlotFormat::Svg), "svg");
    assert_eq!(get_default_plot_extension(PlotFormat::Hpgl), "");
}
