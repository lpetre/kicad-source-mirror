// SPDX-License-Identifier: GPL-3.0-or-later
// Part of the KiCad Rust port (kicad-sexpr oracle harness).  Copyright The KiCad Developers.
// Stub of <eda_item.h> for the kicad-sexpr oracle harness (see ../oracle.cpp): just what
// KICAD_FORMAT::FormatCustomProperties uses.
#pragma once

#include <map>
#include <wx/string.h>

class EDA_ITEM
{
public:
    bool HasCustomProperties() const { return !m_props.empty(); }
    const std::map<wxString, wxString>& GetCustomProperties() const { return m_props; }

    std::map<wxString, wxString> m_props;
};
