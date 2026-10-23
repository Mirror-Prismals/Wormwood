Cue procedure format, schema version 1

A cue contains schema_version, visual, and optionally sound. Visual contains
kind (visual), vertices (xyz and rgb per vertex), and shader. Sound contains
kind (sound) and source. These are appearance data. The Systems provide behavior.
No function names, calls, registry controls, or circuit-breaker settings belong
in a cue. The function registry and System contracts govern execution.

hello_triangle.json is the current shared diagnostic procedure. --cue selects
its name without a directory or extension. --triangle and --av are compatibility
commands selecting this procedure, with sound disabled/enabled respectively.

Procedures are prepared outside the real-time callback. The current file budget
is 256 KiB; each file is validated before Entity creation. The native visual
connection currently accepts three vertex-color vertices. The sound connection
currently accepts diagnostic_sine_440. An unsupported connection capability
causes cancellation and cleanup rather than fallback.
