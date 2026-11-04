# 10: Marks: detect, annotate and target

**What to build:** The Agent detects Marks from the accessibility tree, falling back to OCR when no tree is available. It annotates Observations with numbered Marks and returns the Mark table. Controllers can click or type into a Mark by number or by visible text, so they never compute coordinates.

**Blocked by:** 05

**Status:** ready-for-agent

- [ ] Observation can include an annotated image and a Mark table (number, text, role, bounding box)
- [ ] Marks come from the FakePlatform's accessibility tree when present, otherwise from OCR over the image
- [ ] Actions accept `mark N` or `text "..."` as targets and resolve them through the Coordinate Mapper
- [ ] Targeting a stale Mark (from an older Observation) or ambiguous text fails with a clear error
- [ ] Tests cover tree-based and OCR-based detection with scripted FakePlatform screens
