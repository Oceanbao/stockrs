# Rust Startup Repo

## Resources

- Missing batteries: `https://github.com/brson/stdx`

## Element of Rust

This shows only first error.

```bash
cargo watch -s 'clear; cargo check --tests --color=always 2>&1 | head -40'
```

Precise closure capture.

```rust
fn spawn_threads(config: Arc<Config>) {
    thread::spawn({
        let config = Arc::clone(&config);
        move || do_x(config)
    });

    thread::spawn({
        let config = Arc::clone(&config);
        move || do_y(config)
    });
}
```

Tuple structs and Enum tuple variants as function.

```rust
// create a vector of E::A's using the variant as a constructor function
let v_of_es: Vec<E> = (0..50).map(E::A).collect();

// v_of_es is now vec![A(0), A(1), A(2), A(3), A(4), ..]

// create a vector of Options using Some as a constructor function
let v_of_options: Vec<Option<u64>> = (0..50).map(Some).collect();

struct B(u64);

// create a vector of B's using the struct as a constructor function
let v_of_bs: Vec<B> = (0..50).map(B).collect();
```

Reverse iterator ranges.

```rust
// iterate from 49 to 0
for item in (0..50).rev() {}

// iterate from 50 to 0
for item in (0..=50).rev() {}

// iterate from 50 to 1
for item in (1..=50).rev() {}
```

Pulling first error out of iterator over `Result`

```rust
let results = [Ok(1), Err("nope"), Ok(3), Err("bad")];

let result: Result<Vec<_>, &str> = results.iter().cloned().collect();

// gives us the first error
assert_eq!(Err("nope"), result);

let results = [Ok(1), Ok(3)];

let result: Result<Vec<_>, &str> = results.iter().cloned().collect();

// gives us the list of answers
assert_eq!(Ok(vec![1, 3]), result);
```

Tuple matching.

```rust
let kind = match (args.is_present("bin"), args.is_present("lib")) {
    (true, true) => failure::bail!("can't specify both lib and binary outputs"),
    (false, true) => NewProjectKind::Lib,
    // default to bin
    (_, false) => NewProjectKind::Bin,
};
```

Never.

```rust
// fn () -> ! {} for non-returning (exit or inf-loop).
// Result<T, ()>
enum Never {}

let never = Never:: // oh yeah, can't actually create one...
```

Deactivating mutability - "finalized" object by wrapping it in newtype with private inner value that impl `Deref` but not `DerefMut`.

```rust
mod config {
    #[derive(Clone, Debug, PartialOrd, Ord, Eq, PartialEq)]
    pub struct Immutable<T>(T);

    impl<T> Copy for Immutable<T> where T: Copy {}

    impl<T> std::ops::Deref for Immutable<T> {
        type Target = T;

        fn deref(&self) -> &T {
            &self.0
        }
    }

    #[derive(Default)]
    pub struct Config {
        pub a: usize,
        pub b: String,
    }

    impl Config {
        pub fn build(self) -> Immutable<Config> {
            Immutable(self)
        }
    }

}

use config::Config;

fn main() {
    let mut under_construction = Config {
        a: 5,
        b: "yo".into(),
    }

    under_construction.a = 6;

    let finalized = under_construction.build();

    // at this point, you can make tons of copies,
    // and even if somebody has an owned local version,
    // they won't be able to accidentally change some
    // configuration that
    println!("finalized.a: {}", finalized.a);

    let mut finalized = finalized;

    // the below WON'T work bwahahaha
    // finalized.a = 666;
    // finalized.0.a = 666;
}
```

Shared reference swap trick. `std::cell::Cell` mostly used with `Copy` as `::get` requires `T: Copy`. The following used with non-Copy.

```rust
// Cell::take to impl fmt::Display for iterator. Display has only &self, but need to
// consume the iterator to print it. Cell allows:
use std::{cell::Cell, fmt};

fn display_iter<I>(xs: I) -> impl fmt::Display
where
    I: Iterator,
    I::Item: fmt::Display,
{
    struct IterFmt<I>(Cell<Option<I>>);

    impl<I> fmt::Display for IterFmt<I>
    where
        I: Iterator,
        I::Item: fmt::Display,
    {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            // Advanced Jones's trick: `mem::replace` with `&` reference.
            let xs: Option<I> = self.0.take();
            let xs: I = xs.unwrap();

            let mut first = true;
            for item in xs {
                if !first {
                    f.write_str(", ")?
                }
                first = false;
                fmt::Display::fmt(&item, f)?
            }

            Ok(())
        }
    }

    IterFmt(Cell::new(Some(xs)))
}

fn main() {
    let xs = vec![1, 2, 3].into_iter();
    assert_eq!(display_iter(xs).to_string(), "1, 2, 3");
}
```

## Web App Workflow (`axum`, `sqlx`)

- Install `sqlx-cli` and start local dev with new database and migrations.
- Use offline mode `query!` macros API in code.
- In dev, start a local postgres server to run migration against.
- Set env `DATABASE_URL` to local and `SQLX_OFFLINE` to true when checking.
- Must ensure `cargo sqlx prepare` to write offline checking files and git control them.
- Once commited and pushed, the code is safe to build offline, simple build in CICD.
- A generate `build.rs` script is used to watch changes in `/migrations` files if no code change.
- In prod, need to set `DATABASE_URL` to a live server to enable `migrate!` macro upon init.
