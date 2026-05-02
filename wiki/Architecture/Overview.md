# Architecture Overview

WormHole has three pieces: the Rust binary that runs the agent loop, the BrainWorms dashboard that shows you what the agent is doing, and the workspace that holds memory, tools, skills, and modules. They share three contracts: HTTP between the binary and dashboard, filesystem between the binary and workspace, and JSON snapshots between the workspace and dashboard. This page draws the box-and-arrow diagram, names every file you should know about, and links to the deep-dive pages for each piece.

<!-- TODO: fill in during Phase 1 docs sprint -->
