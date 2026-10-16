# 10. Planning mode (coming in this build)

Planning mode predicts coverage for a building you have not measured, or for access points you have not
installed yet: you draw walls, place virtual access points and OpenSpot calculates the expected signal.

**This page is deliberately short.** The Planning interface is being built at the same time as this guide, and the guide only
describes screens that are known to exist. When the interface lands in a tagged build, this page will be
expanded with a walkthrough.

## What exists today

The **calculation engine** is implemented, tested and documented, but it is a library: it has no screens yet in the
version this guide describes. [PLANNING_ENGINE.md](../PLANNING_ENGINE.md) describes it:

- a catalogue of wall and floor materials (absorption and reflection derived from the ITU-R P.2040 material model);
- multi-wall signal prediction in the style of ITU-R P.1238, including floors and cutouts;
- calibrating the model against your real measurements, so predictions in the same building match what you measured;
- an optional single-bounce ray mode;
- suggesting access point positions that improve coverage;
- antenna patterns in a JSON format.

## What does not exist

Check [Limits](11-limits.md) and [ROADMAP.md](../ROADMAP.md) for the current status of each Planning item. In particular a router
catalogue is not provided, and vendor antenna-file formats are not read.

## Why it is documented separately

Predictions are only as good as the walls and materials you give them. Calibrate against measurements
(the engine supports it) before trusting a prediction, and keep in mind that prediction has not yet been validated against a
real building; roadmap item Q-01 covers that.
