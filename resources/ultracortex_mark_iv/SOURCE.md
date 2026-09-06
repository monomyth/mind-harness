# Ultracortex Mark IV medium frame (vendored)

Official public STLs (OpenBCI Docs `assets/MarkIV/STL_Directory`):

- https://github.com/OpenBCI/Docs/raw/master/assets/MarkIV/STL_Directory/M4_Medium_Front.stl
- https://github.com/OpenBCI/Docs/raw/master/assets/MarkIV/STL_Directory/M4_Medium_Back.stl

Same files: OpenBCI/Ultracortex `Mark_IV/MarkIV-FINAL/STL_Directory`.

- `M4_Medium_Front.stl` / `M4_Medium_Back.stl` — original medium print halves (~50k tris each).
- `frame.bin` — front+back merged, bbox-centered, X flipped so +X is right (Fp1 left),
  vertex-cluster decimated (~8k tris) for Head Plot orbit.

Units: max radius 1. Axes: +X right, −Y anterior, +Z up.

The Mark IV frame has 35 circular INSERT sockets (nodes), not decorative lattice
openings. Node sites follow the 10–20 system. Centers come from official
`M4H6_Medium Node Array.stl` (35 node solids), transformed into `frame.bin` space.

OpenBCI’s default maps, from the Mark IV docs. The board mount sits at the
back of the frame. Remaining inserts stay empty. Decorative lattice openings
are unlabeled.

Cyton 8 (GUI N1P–N8P):

| Ch | Site | On the headset |
| --- | --- | --- |
| 1 | Fp1 | left forehead (front pair; flat units) |
| 2 | Fp2 | right forehead (front pair; flat units) |
| 3 | C3 | left central |
| 4 | C4 | right central |
| 5 | P7 | left, behind the ear |
| 6 | P8 | right, behind the ear |
| 7 | O1 | left occipital, lowest back beside the board |
| 8 | O2 | right occipital, lowest back beside the board |

Cyton Daisy 16 keeps the eight above and adds Daisy N1P–N8P (GUI 9–16):

| Ch | Site | On the headset |
| --- | --- | --- |
| 9 | F7 | left outer frontal |
| 10 | F8 | right outer frontal |
| 11 | F3 | left inner frontal (behind Fp1, toward the crown) |
| 12 | F4 | right inner frontal (behind Fp2, toward the crown) |
| 13 | T7 | left temporal (over the ear, beside C3) |
| 14 | T8 | right temporal (over the ear, beside C4) |
| 15 | P3 | left parietal (inboard of P7, above O1) |
| 16 | P4 | right parietal (inboard of P8, above O2) |

https://docs.openbci.com/AddOns/Headwear/MarkIV/#electrode-location-overview
8-channel wiring: https://docs.openbci.com/assets/images/ultracortex-connect-wiring-5be480f9d516a974a8b08eabbeace92a.jpg
16-channel add-on: https://docs.openbci.com/AddOns/Headwear/MarkIV/#16-channel-add-ons
