// Copyright (c) 2016 Sergio Benitez
// Copyright (c) 2021 Cognite AS
//! Rocket extension to permit enums in application/x-www-form-urlencoded forms
//! This crate is a workaround for [https://github.com/SergioBenitez/Rocket/issues/1937](rocket#1937).
//!
//! It is derived from the included [serde_json](`rocket::serde::json`]) implementation in rocket.
//!
//! ```rust
//! # use rocket_enumform::UrlEncoded;
//! # use serde::Deserialize;
//! # use rocket::post;
//! #[derive(Debug, Deserialize)]
//! #[serde(tag = "type")]
//! enum Body {
//!     #[serde(rename = "variant_one")]
//!     VariantOne(VariantOne),
//!     #[serde(rename = "variant_two")]
//!     VariantTwo(VariantTwo),
//! }
//!
//! #[derive(Debug, Deserialize)]
//! struct VariantOne {
//!     content_one: String
//! }
//!
//! #[derive(Debug, Deserialize)]
//! struct VariantTwo {
//!     content_two: String
//! }
//!
//! #[post("/form", format = "form", data = "<data>")]
//! fn body(data: UrlEncoded<Body>) -> String { format!("{:?}", data) }
//! ```
//! ## status
//!
//! Works but not tested, nor have local testing affordances been added yet.
//!
// # Testing
//
// TODO; idea is use the underlying serde_urlencoded serializer and implement the glue
// needed as extension traits.
//
// ///The [`LocalRequest`] and [`LocalResponse`] types provide [`json()`] and
// ///[`into_json()`] methods to create a request with serialized JSON and
// ///deserialize a response as JSON, respectively.
//
// ///[`LocalRequest`]: crate::local::blocking::LocalRequest [`LocalResponse`]:
// ///crate::local::blocking::LocalResponse [`json()`]:
// ///crate::local::blocking::LocalRequest::json() [`into_json()`]:
// ///crate::local::blocking::LocalResponse::into_json()

use std::ops::{Deref, DerefMut};
use std::{error, fmt, io};

use rocket::data::{Data, FromData, Limits, Outcome};
use rocket::error_;
use rocket::form::prelude as form;
use rocket::http::uri::fmt::{Formatter as UriFormatter, FromUriParam, Query, UriDisplay};
use rocket::http::{ContentType, Status};
use rocket::request::{local_cache, Request};
use rocket::response::{self, content, Responder};
use serde::{Deserialize, Serialize};

/// The UrlEncoded guard: easily consume x-www-form-urlencoded requests.
///
/// ## Receiving
///
/// `UrlEncoded` is both a data guard and a form guard.
///
/// ### Data Guard
///
/// To deserialize request body data from x-www-form-urlencoded, add a `data`
/// route argument with a target type of `UrlEncoded<T>`, where `T` is some type
/// you'd like to parse. `T` must implement [`serde::Deserialize`]. See
/// [`serde_urlencoded`](serde_urlencoded) docs on the flatten-workaround for important hints about
/// more complex datatypes.
///
/// ```rust
/// # #[macro_use] extern crate rocket;
/// #
/// # type User = usize;
/// use rocket_enumform::UrlEncoded;
///
/// #[post("/user", format = "form", data = "<user>")]
/// fn new_user(user: UrlEncoded<User>) {
///     /* ... */
/// }
/// ```
///
/// You don't _need_ to use `format = "form"`, but it _may_ be what you
/// want. Using `format = urlencoded` means that any request that doesn't
/// specify "application/x-www-form-urlencoded" as its `Content-Type` header
/// value will not be routed to the handler.
///
/// ### Incoming Data Limits
///
/// The default size limit for incoming UrlEncoded data is the built in form
/// limit. Setting a limit protects your application from denial of service
/// (DoS) attacks and from resource exhaustion through high memory consumption.
/// The limit can be increased by setting the `limits.form` configuration
/// parameter. For instance, to increase the UrlEncoded limit to 5MiB for all
/// environments, you may add the following to your `Rocket.toml`:
///
/// ```toml
/// [global.limits]
/// form = 5242880
/// ```
#[repr(transparent)]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UrlEncoded<T>(pub T);

/// Error returned by the [`UrlEncoded`] guard when deserialization fails.
#[derive(Debug)]
pub enum Error<'a> {
    /// An I/O error occurred while reading the incoming request data.
    Io(io::Error),

    /// The client's data was received successfully but failed to parse as valid
    /// UrlEncoded or as the requested type. The `&str` value in `.0` is the raw data
    /// received from the user, while the `Error` in `.1` is the deserialization
    /// error from `serde`.
    Parse(&'a str, ::serde_urlencoded::de::Error),
}

impl<'a> fmt::Display for Error<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "i/o error: {}", err),
            Self::Parse(_, err) => write!(f, "parse error: {}", err),
        }
    }
}

impl<'a> error::Error for Error<'a> {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Parse(_, err) => Some(err),
        }
    }
}

impl<T> UrlEncoded<T> {
    /// Consumes the UrlEncoded wrapper and returns the wrapped item.
    ///
    /// # Example
    /// ```rust
    /// use rocket_enumform::UrlEncoded;
    /// let string = "Hello".to_string();
    /// let outer = UrlEncoded(string);
    /// assert_eq!(outer.into_inner(), "Hello".to_string());
    /// ```
    #[inline(always)]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<'r, T: Deserialize<'r>> UrlEncoded<T> {
    fn from_str(s: &'r str) -> Result<Self, Error<'r>> {
        ::serde_urlencoded::from_str(s)
            .map(UrlEncoded)
            .map_err(|e| Error::Parse(s, e))
    }

    async fn from_data(req: &'r Request<'_>, data: Data<'r>) -> Result<Self, Error<'r>> {
        let limit = req.limits().get("form").unwrap_or(Limits::FORM);
        let string = match data.open(limit).into_string().await {
            Ok(s) if s.is_complete() => s.into_inner(),
            Ok(_) => {
                let eof = io::ErrorKind::UnexpectedEof;
                return Err(Error::Io(io::Error::new(eof, "data limit exceeded")));
            }
            Err(e) => return Err(Error::Io(e)),
        };

        Self::from_str(local_cache!(req, string))
    }
}

#[rocket::async_trait]
impl<'r, T: Deserialize<'r>> FromData<'r> for UrlEncoded<T> {
    type Error = Error<'r>;

    async fn from_data(req: &'r Request<'_>, data: Data<'r>) -> Outcome<'r, Self> {
        match Self::from_data(req, data).await {
            Ok(value) => Outcome::Success(value),
            Err(Error::Io(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
                Outcome::Failure((Status::PayloadTooLarge, Error::Io(e)))
            }
            Err(Error::Parse(s, e)) => {
                error_!("{:?}", e);
                Outcome::Failure((Status::UnprocessableEntity, Error::Parse(s, e)))
            }
            Err(e) => Outcome::Failure((Status::BadRequest, e)),
        }
    }
}

/// Serializes the wrapped value into UrlEncoding. Returns a response with Content-Type
/// application/x-www-form-urlencode and a fixed-size body with the serialized value. If serialization
/// fails, an `Err` of `Status::InternalServerError` is returned.
impl<'r, T: Serialize> Responder<'r, 'static> for UrlEncoded<T> {
    fn respond_to(self, req: &'r Request<'_>) -> response::Result<'static> {
        let string = ::serde_urlencoded::to_string(&self.0).map_err(|e| {
            error_!("UrlEncoding failed to serialize: {:?}", e);
            Status::InternalServerError
        })?;

        content::Custom(ContentType::Form, string).respond_to(req)
    }
}

impl<T: Serialize> UriDisplay<Query> for UrlEncoded<T> {
    fn fmt(&self, f: &mut UriFormatter<'_, Query>) -> fmt::Result {
        let string = ::serde_urlencoded::to_string(&self.0).map_err(|_| fmt::Error)?;
        f.write_value(&string)
    }
}

macro_rules! impl_from_uri_param_from_inner_type {
    ($($lt:lifetime)?, $T:ty) => (
        impl<$($lt,)? T: Serialize> FromUriParam<Query, $T> for UrlEncoded<T> {
            type Target = UrlEncoded<$T>;

            #[inline(always)]
            fn from_uri_param(param: $T) -> Self::Target {
                UrlEncoded(param)
            }
        }
    )
}

impl_from_uri_param_from_inner_type!(, T);
impl_from_uri_param_from_inner_type!('a, &'a T);
impl_from_uri_param_from_inner_type!('a, &'a mut T);

rocket::http::impl_from_uri_param_identity!([Query] (T: Serialize) UrlEncoded<T>);

impl<T> From<T> for UrlEncoded<T> {
    fn from(value: T) -> Self {
        UrlEncoded(value)
    }
}

impl<T> Deref for UrlEncoded<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for UrlEncoded<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl From<Error<'_>> for form::Error<'_> {
    fn from(e: Error<'_>) -> Self {
        match e {
            Error::Io(e) => e.into(),
            Error::Parse(_, e) => form::Error::custom(e),
        }
    }
}

#[rocket::async_trait]
impl<'v, T: Deserialize<'v> + Send> form::FromFormField<'v> for UrlEncoded<T> {
    fn from_value(field: form::ValueField<'v>) -> Result<Self, form::Errors<'v>> {
        Ok(Self::from_str(field.value)?)
    }

    async fn from_data(f: form::DataField<'v, '_>) -> Result<Self, form::Errors<'v>> {
        Ok(Self::from_data(f.request, f.data).await?)
    }
}

/// Deserialize an instance of type `T` from bytes of UrlEncoded text.
///
/// **_Always_ use [`UrlEncoded`] to deserialize UrlEncoded request data.**
///
/// # Example
///
/// ```
/// use rocket::serde::Deserialize;
///
/// #[derive(Debug, PartialEq, Deserialize)]
/// struct Data<'r> {
///     framework: &'r str,
///     stars: usize,
/// }
///
/// let bytes = br#"framework=Rocket&stars=5"#;
///
/// let data: Data = rocket_enumform::from_slice(bytes).unwrap();
/// assert_eq!(data, Data { framework: "Rocket", stars: 5, });
/// ```
///
/// # Errors
///
/// This conversion can fail if the structure of the input does not match the
/// structure expected by `T`, for example if `T` is a struct type but the input
/// contains something other than a UrlEncoded map. It can also fail if the structure
/// is correct but `T`'s implementation of `Deserialize` decides that something
/// is wrong with the data, for example required struct fields are missing from
/// the UrlEncoded map or some number is too big to fit in the expected primitive
/// type.
#[inline(always)]
pub fn from_slice<'a, T>(slice: &'a [u8]) -> Result<T, ::serde_urlencoded::de::Error>
where
    T: Deserialize<'a>,
{
    ::serde_urlencoded::from_bytes(slice)
}

/// Deserialize an instance of type `T` from a string of UrlEncoded text.
///
/// **_Always_ use [`UrlEncoded`] to deserialize UrlEncoded request data.**
///
/// # Example
///
/// ```
/// use rocket::serde::Deserialize;
///
/// #[derive(Debug, PartialEq, Deserialize)]
/// struct Data<'r> {
///     framework: &'r str,
///     stars: usize,
/// }
///
/// let string = r#"framework=Rocket&stars=5"#;
///
/// let data: Data = rocket_enumform::from_str(string).unwrap();
/// assert_eq!(data, Data { framework: "Rocket", stars: 5 });
/// ```
///
/// # Errors
///
/// This conversion can fail if the structure of the input does not match the
/// structure expected by `T`, for example if `T` is a struct type but the input
/// contains something other than a UrlEncoded map. It can also fail if the structure
/// is correct but `T`'s implementation of `Deserialize` decides that something
/// is wrong with the data, for example required struct fields are missing from
/// the UrlEncoded map or some number is too big to fit in the expected primitive
/// type.
#[inline(always)]
pub fn from_str<'a, T>(string: &'a str) -> Result<T, ::serde_urlencoded::de::Error>
where
    T: Deserialize<'a>,
{
    ::serde_urlencoded::from_str(string)
}
