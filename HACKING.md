# Hacking

## install rust & cargo

https://rustup.rs/

## compilation

### run in debug

```
$ cargo run -- <master.csv> <partner.csv> <result.csv>
```

### continuous compilation

(once) Install [cargo watch](https://github.com/passcod/cargo-watch)
```
cargo install cargo-watch
```

(each time)
```
cargo watch
```


## release

```
cargo build --release
```

### cross-compilation to windows
(only possible on linux systems)

(once) install [cross](https://github.com/japaric/cross)
```
$ cargo install cross
```

Then:
```
cross build --release --target x86_64-pc-windows-gnu
```
