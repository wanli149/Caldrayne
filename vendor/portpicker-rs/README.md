# THIS FORK CONTAINS THE IPV6 FIX FROM wusyong AND my DEPENDENCY FIX

# Portpicker-rs - Pick a free unused port

Picks a free port, that is unused on both TCP and UDP.

Usage:

```rust
use portpicker::pick_unused_port;
let port: u16 = pick_unused_port().expect("No ports free");
```
