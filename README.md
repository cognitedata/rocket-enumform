# Rocket extension to permit enums in application/x-www-form-urlencoded forms

This crate is a workaround for [https://github.com/SergioBenitez/Rocket/issues/1937](rocket#1937).

It is derived from the included serde_json implementation in rocket.

```rust
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Body {
    #[serde(rename = "variant_one")]
    VariantOne(VariantOne),
    #[serde(rename = "variant_two")]
    VariantTwo(VariantTwo),
}

#[derive(Debug, Deserialize)]
struct VariantOne {
    content_one: String
}

#[derive(Debug, Deserialize)]
struct VariantTwo {
    content_two: String
}

#[post("/form", format = "form", data = "<data>")]
fn body(data: UrlEncoded<Body>) -> String { /*...*/ }
```

## status

Works but not unit tested, nor have local testing affordances for users been added yet.

Supports rust stable and nightly, matching Rocket.

## Code of conduct

Please note that this project is released with a Contributor Code of Conduct. By
participating in this project you agree to abide by its terms.

## Contributing

PR's on Github as normal please. Cargo test and rustfmt code before submitting. 