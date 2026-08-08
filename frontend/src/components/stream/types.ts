// Local, editable form state for the Stream Settings page. Numeric text
// fields are kept as strings (like Rtsp.tsx's maxClients) so the input can be
// cleared/typed-into freely; StreamSettings.tsx converts to native types only
// when building the PUT patch. See rust/octocam-web/src/settings.rs for the
// server-side field names/ranges this mirrors.
export interface StreamFormState {
  cameraEnabled: boolean

  resolution: string
  framerate: string
  bitrateKbps: string

  subStreamEnabled: boolean
  subResolution: string
  subFramerate: string
  subBitrateKbps: string

  rotation: number
  contrast: string
  brightness: string
  hflip: boolean
  vflip: boolean
  noirMode: boolean

  motionEnabled: boolean
  motionSensitivity: string
  // A u64 bitmask. Kept as BigInt end-to-end in the UI — Number can't
  // represent all 64 bits without precision loss (see api.ts's
  // Settings.motion_zones comment). Serialized to a decimal string only when
  // building the PUT patch.
  motionZones: bigint

  hksvEnabled: boolean

  textOverlayEnabled: boolean
  textOverlayTimezone: string
  textOverlayDateFormat: string
  textOverlayClockFormat: string

  timeServer: string
}

export type StreamFormPatch = Partial<StreamFormState>
