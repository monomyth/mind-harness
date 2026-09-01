# Ultracortex Mark IV medium frame (vendored)

Official public STLs (OpenBCI Docs `assets/MarkIV/STL_Directory`):

- https://github.com/OpenBCI/Docs/raw/master/assets/MarkIV/STL_Directory/M4_Medium_Front.stl
- https://github.com/OpenBCI/Docs/raw/master/assets/MarkIV/STL_Directory/M4_Medium_Back.stl

Same files: OpenBCI/Ultracortex `Mark_IV/MarkIV-FINAL/STL_Directory`.

- `M4_Medium_Front.stl` / `M4_Medium_Back.stl` — original medium print halves (~50k tris each).
- `frame.bin` — front+back merged, bbox-centered, X flipped so +X is right (Fp1 left),
  vertex-cluster decimated (~8k tris) for Head Plot orbit.

Units: max radius 1. Axes: +X right, −Y anterior, +Z up.

The 35 named holes are the circular INSERT sockets (electrode nodes), not decorative
lattice openings. Centers come from official `M4H6_Medium Node Array.stl` (35 node
solids), transformed into `frame.bin` space. Names are 10-20/10-10 by anatomy (Fp1 left-front forehead,
O posterior, Cz up). Decorative lattice openings are unlabeled.
