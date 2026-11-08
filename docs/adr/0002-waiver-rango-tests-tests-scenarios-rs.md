---
status: accepted
date: 2026-11-08
---
# Waiver: write rango-tests/tests/scenarios.rs

## Context and Problem Statement

Requested by the agent in session 18e6046ddf26a105-3fe-0:

C5 requires the scenario suite to cover dump/restore ("toute la surface du produit — … bulk, dump/restore"), which the current protected test does not. I am ADDING a new test function (blog_app_dump_restore_roundtrip) that exercises the real `rango` binary's dump/restore; I am not modifying the existing blog_app_end_to_end test.

## Decision Outcome

The human waived the protection of `rango-tests/tests/scenarios.rs` for this session only, until 2026-11-08 11:41 UTC. Granted through fennec's UI; an agent cannot grant itself a waiver.
