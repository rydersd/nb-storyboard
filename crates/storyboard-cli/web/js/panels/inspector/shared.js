// Inspector view state shared by its host, sections and fields (UI only; never project data).

export const insp = {
  key: null,                // what the inspector shows: 'summary', 'shot:ID', 'sticky:ID' or null
  fields: new Map(),        // field key -> field record (see makeField)
  parts: null,              // elements the shot sections share (seq, preview, layerBox, ...)
  sections: [],             // mounted shot sections: {def, slot, handle}
  sigs: {},
  previewUrl: '',
  previewObjectUrl: '',
  layersOpen: false,
  replyDrafts: new Map(),
  newSticky: { text: '', kind: 'note', pin: 'shot' },
};
