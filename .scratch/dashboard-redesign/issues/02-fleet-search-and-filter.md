# 02: Search and status filter on the fleet list

**What to build:** An Operator can find Devices on the fleet list by typing part of a name and by choosing a Device Status (Online, Offline, Waking). Both live in the URL, so pressing Back from a Device returns to the same filtered list and a link can be shared. The table, its columns and the Online/Offline/Waking counts stay as they are. See spec stories 8–14.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Search box filters Devices by name (case-insensitive, substring)
- [ ] Status filter offers All, Online, Offline, Waking
- [ ] Search text and status are read from and written to the URL (`q`, `status`); opening such a URL directly shows the filtered list
- [ ] Back from a Device restores the filtered list
- [ ] Counts of Online/Offline/Waking stay visible and reflect the whole fleet
- [ ] When nothing matches, a message says no Device matches (distinct from "Chưa có Device nào.")
- [ ] Clicking a row still opens the Device
- [ ] Playwright: search narrows the list; status filter narrows the list; filtered URL survives Back; no-match message
