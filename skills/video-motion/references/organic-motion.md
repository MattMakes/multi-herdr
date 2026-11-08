# Organic motion, impacts and plate scale

## Organic chains

Use FABRIK inverse kinematics for a tentacle, tail, rope or cloth chain. Use 40 or more joints and run several FABRIK iterations for each frame. A low joint count makes visible notches near the root.

Add 2 or more sine waves along the solved chain. Give the waves different frequencies, amplitudes and phases. Derive every phase from scene time so that frame seeking stays deterministic. Keep the sine displacement smaller near a fixed root.

## Impacts with a result

Each impact must change the scene. For example, each blow can complete 1 part of a logo. Do not use repeated impacts that leave the subject and scene unchanged.

## Plate scale

Do not draw a raster plate above 1.2x its natural width or height. Export plate frames at 2x when a camera move or reframe needs more scale.

Write `raster-sizes.json` during the render. Run this gate:

```sh
python3 "<skill-dir>/scripts/scan_linear_motion.py" --raster-sizes render/raster-sizes.json
```

The command exits `1` when any sample is above 1.2x. It exits `2` when the report is missing or invalid.
