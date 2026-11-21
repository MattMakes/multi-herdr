# A failed skill install never echoes its source spec

`skillscmd::install` (`crates/horch/src/cmd/skillscmd.rs`) reports an
install failure without the source spec, because a rejected spec can carry
a credential, such as a token in a git URL.
