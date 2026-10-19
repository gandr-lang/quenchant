# Duplication fixture

The README the duplication fixture compares its documentation with.

The frame reader keeps one buffer per connection, drains each buffer in arrival order and hands every complete frame to the [`Router`](../duplication.rs), which chooses its handler from the frame's kind and never blocks the reader.
