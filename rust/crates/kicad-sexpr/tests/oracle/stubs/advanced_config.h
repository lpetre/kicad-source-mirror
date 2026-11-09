// SPDX-License-Identifier: GPL-3.0-or-later
// Part of the KiCad Rust port (kicad-sexpr oracle harness).  Copyright The KiCad Developers.
// Stub of <advanced_config.h> for the kicad-sexpr oracle harness (see ../oracle.cpp).
#pragma once

class ADVANCED_CFG
{
public:
    static const ADVANCED_CFG& GetCfg()
    {
        static ADVANCED_CFG cfg;
        return cfg;
    }

    bool m_CompactSave = false;
};
