# Tower Fallthrough Filter

Ever wanted to filter a Tower Service, but not abort the request when the filter doesn't match? Now you can!

## Installation

Add this to your `Cargo.toml`:
```toml
[dependencies]
tower-fallthrough-filter = "*"
```
Or add it using the `cargo` command:
```sh
cargo add tower-fallthrough-filter
```

## Example how to use

```rust
use tower_fallthrough_filter::{Filter, FilterLayer};

#[derive(Clone)]
struct MyFilter;

impl<T> Filter<T> for MyFilter {
    fn filter(&self, _: T) -> bool {
        // This can be any logic you want
        // and can depend on some state stored
        // in the filter itself.

        // When this function returns true, my_service
        // will be called, otherwise the request will
        // fall through to the next service.
        some_buisness_logic()
    }
}

let my_service = MyService::new();
let layer = FilterLayer::new(MyFilter, my_service);

// Now you can use the layer as a normal Tower Layer
```

Check the examples folder for more examples.