// This program source code file is part of KiCad, a free EDA CAD application.
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// JSON shapes of the router API (package WASM-API-1, rust/docs/WASM_API.md). Embedded into
// the wasm-bindgen output (pkg/kicad_wasm.d.ts) by src/board_api.rs. Units: nanometres
// (integers), degrees; layers are canonical names ("F.Cu", "In1.Cu", "Edge.Cuts").

/** `[x, y]` in nm. */
export type Pt = [number, number];
/** `[x, y, w, h]` in nm (BOX2I position and size). */
export type Box = [number, number, number, number];
/** Numeric item handle, valid for this Board until the item is removed. */
export type Handle = number;

/** One polygon of a SHAPE_POLY_SET. */
export interface Polygon { outline: Pt[]; holes?: Pt[][]; width?: number }

export interface CopperLayer { name: string; userName: string; layerType: string; layerId: number; depth: number }
export interface RuleMinimums {
    minClearance: number; trackMinWidth: number; minConnection: number;
    viasMinAnnularWidth: number; viasMinSize: number; minThroughDrill: number;
    microViasMinSize: number; microViasMinDrill: number; copperEdgeClearance: number;
    holeClearance: number; holeToHoleMin: number; maxError: number;
}
export interface StackupLayer { name: string; typeName: string; layer: string | undefined; thickness: number }
export interface BoardInfo {
    name: string; copperLayerCount: number; copperLayers: CopperLayer[]; enabledLayers: string[];
    outline: Polygon[]; outlineOk: boolean; bbox: Box; edgesBbox: Box; rules: RuleMinimums;
    boardThickness: number; stackup: StackupLayer[];
}

export interface NetDetail { code: number; name: string; netclass: string; netclasses: string[] }
export interface NetclassInfo {
    name: string; priority: number; clearance: number | undefined; trackWidth: number | undefined;
    viaDiameter: number | undefined; viaDrill: number | undefined; microviaDiameter: number | undefined;
    microviaDrill: number | undefined; diffPairWidth: number | undefined; diffPairGap: number | undefined;
    diffPairViaGap: number | undefined;
}

export interface DrillInfo { shape: 'CIRCLE' | 'OBLONG'; size: Pt }
export interface PadShapeInfo {
    layer: string; shape: string; size: Pt; offset: Pt; cornerRadius: number;
    chamferRatio: number; chamferPositions: number;
}
export interface PadPolygon { layer: string; flashed: boolean; polygon: Polygon[] }
export interface PadInfo {
    id: Handle; uuid: string; footprint: Handle; footprintUuid: string; reference: string;
    number: string; net: number; position: Pt; orientation: number; layers: string[];
    attribute: 'PTH' | 'SMD' | 'CONN' | 'NPTH'; drill: DrillInfo | undefined; shapes: PadShapeInfo[];
    polygons?: PadPolygon[]; teardrops: boolean; locked: boolean;
}
export interface FootprintInfo {
    id: Handle; uuid: string; reference: string; value: string; position: Pt; orientation: number;
    side: 'F.Cu' | 'B.Cu'; courtyardFront: Polygon[]; courtyardBack: Polygon[]; bbox: Box;
    locked: boolean; dnp: boolean; pads: Handle[];
}

/** `from_board` | `yes` | `no`. */
export type ViaMode = 'from_board' | 'yes' | 'no';
export interface ViaFinish {
    tenting: [ViaMode, ViaMode]; covering: [ViaMode, ViaMode]; plugging: [ViaMode, ViaMode];
    capping: ViaMode; filling: ViaMode;
}
export interface TeardropParams {
    enabled: boolean; maxLen: number; maxWidth: number; bestLengthRatio: number;
    bestWidthRatio: number; widthToSizeFilterRatio: number; curvedEdges: boolean;
    allowTwoTracks: boolean; onPadsInZones: boolean;
}
export type ViaType = 'through' | 'blind' | 'buried' | 'micro';
export type UnconnectedLayerMode = 'KEEP_ALL' | 'START_END_ONLY' | 'REMOVE_ALL' | 'REMOVE_EXCEPT_START_AND_END';
export interface ViaInfo {
    viaType: ViaType; top: string; bottom: string; diameters: [string, number][]; drill: number;
    free: boolean; finish: ViaFinish; unconnectedLayers: UnconnectedLayerMode;
    zoneLayerConnections: string[]; teardrops: TeardropParams;
}
export interface RouteItem {
    id: Handle; uuid: string; kind: 'track' | 'arc' | 'via'; net: number; start: Pt; end: Pt;
    mid?: Pt; width: number; layer: string; locked: boolean; solderMask?: boolean;
    solderMaskMargin?: number; via?: ViaInfo;
}

export interface Keepouts { tracks: boolean; vias: boolean; pads: boolean; copperPour: boolean; footprints: boolean }
export interface ZoneFill { layer: string; polygons: Polygon[] }
export interface ZoneInfo {
    id: Handle; uuid: string; footprint?: Handle; name: string; net: number; layers: string[];
    priority: number; ruleArea: boolean; keepout: Keepouts; teardrop: boolean; locked: boolean;
    clearance: number | undefined; minThickness: number; connection: string; islandMode: string;
    outline: Polygon[]; filled: boolean; fills?: ZoneFill[];
}
export interface ObstacleInfo {
    id: Handle; uuid: string; kind: string; layer: string; net: number; footprint?: Handle;
    polygon: Polygon[];
}

export interface RatsnestEdge { net: number; source: Handle; target: Handle; sourcePos: Pt; targetPos: Pt }
export interface NetConnection { net: number; pads: number; unconnected: number; clusters: Handle[][] }
export interface ConnectivityInfo { unconnected: number; edges: RatsnestEdge[]; nets: NetConnection[] }

/** A board item (`id`) or a bare net (`net`, evaluated on a dummy track or via). */
export interface ItemSel { id?: Handle; net?: number; dummy?: 'track' | 'via' }
export interface ConstraintQuery { kind: string; a: ItemSel; b?: ItemSel; layer: string }
export interface ConstraintResult {
    constraintType: string; min: number | undefined; opt: number | undefined; max: number | undefined;
    name: string; minRule: string | undefined; disallow: number; implicitMin: boolean;
    severity: string; severityCode: number;
}
export interface RouterSizes {
    clearance: number; trackWidth: number; viaDiameter: number; viaDrill: number;
    microviaDiameter: number; microviaDrill: number;
}

export interface NewTrack {
    start: Pt; end: Pt; width?: number; layer: string; net?: number; uuid?: string;
    locked?: boolean; solderMask?: boolean; solderMaskMargin?: number;
}
export interface NewArc extends NewTrack { mid: Pt }
export interface NewVia {
    at: Pt; viaType?: ViaType; top?: string; bottom?: string; diameter?: number; drill?: number;
    net?: number; free?: boolean; uuid?: string; locked?: boolean; finish?: ViaFinish;
    unconnectedLayers?: UnconnectedLayerMode; zoneLayerConnections?: string[];
    teardrops?: TeardropParams;
}
export type NewItem = ({ kind: 'track' } & NewTrack) | ({ kind: 'arc' } & NewArc) | ({ kind: 'via' } & NewVia);
export interface ItemChange {
    id: Handle; start?: Pt; end?: Pt; mid?: Pt; width?: number; drill?: number; layer?: string;
    bottom?: string; net?: number; locked?: boolean; free?: boolean;
}
export interface RemoveRouting { nets?: number[]; keepLocked?: boolean; vias?: boolean }
export interface EditBatch { remove?: Handle[]; removeRouting?: RemoveRouting; change?: ItemChange[]; add?: NewItem[] }
export interface EditResult { added: Handle[]; addedUuids: string[]; removed: Handle[]; changed: Handle[] }

export interface TeardropSettings {
    targetVias: boolean; targetPthPads: boolean; targetSmdPads: boolean;
    useRoundShapesOnly: boolean; round: TeardropParams; rect: TeardropParams; track: TeardropParams;
}
export interface TeardropOutcome { teardrops: number; candidates: number; without: Handle[] }

export interface ViolationItem { uuid: string; id: Handle | undefined }
export interface Violation {
    violationType: string; code: number; severity: string; description: string; position: Pt;
    layer: string; items: ViolationItem[]; rule: string | undefined;
    section: 'marker' | 'unconnected' | 'parity';
}
export interface DrcSummary {
    violations: Violation[]; markers: number; unconnected: number; parity: number;
    checksNotRun: string[];
}
export interface QuickDrcOptions { providers?: string[]; allTrackErrors?: boolean; severities?: string[] }
export interface ReadOptions { polygons?: boolean }

// LIVE-1: transactions (BOARD_COMMIT::Push with incremental connectivity), checkpoints and
// the flat record forms (docs/WASM_API.md "Live router loop").
export interface NetChange { id: Handle; from: number; to: number }
export interface CommitReport extends EditResult {
    dirtyNets: number[]; netUnconnected: [number, number][]; unconnected: number;
    unconnectedVisible: number; netChanges: NetChange[];
}
/** `commitBulk`: handles as Float64Array, `netUnconnected` as [net, count] pairs,
 *  `netChanges` as [id, from, to] triples. */
export interface CommitReportBulk {
    added: Float64Array; removed: Float64Array; changed: Float64Array; dirtyNets: Int32Array;
    netUnconnected: Int32Array; netChanges: Float64Array; unconnected: number;
    unconnectedVisible: number;
}
/** Flat records: tracks (x1, y1, x2, y2, width, layer, net), arcs (+ midX, midY),
 *  vias (x, y, diameter, drill, top, bottom, net, type 0 through / 1 blind / 2 buried /
 *  3 micro); layers are PCB_LAYER_ID values (F.Cu 0, B.Cu 2, In1.Cu 4, ...). */
export interface RouteRecords {
    trackIds: Float64Array; tracks: Int32Array; arcIds: Float64Array; arcs: Int32Array;
    viaIds: Float64Array; vias: Int32Array;
}
