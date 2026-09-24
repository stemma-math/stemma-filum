//! Enums whose variants each have one fixed spelling.

/// Implements `as_str` and `Display` for a fieldless enum, spelling each
/// variant as given. With an error type and message it also implements
/// `FromStr`, ignoring surrounding whitespace; the message may use `{0}`
/// for the rejected input.
///
/// The spellings are what goes on the wire, in the database and in the
/// interface, so they must match the enum's serde names.
macro_rules! spelled {
    ($type:ident { $($variant:ident => $word:literal),+ $(,)? }) => {
        impl $type {
            /// The word it is stored, sent and shown as.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $word),+
                }
            }
        }

        impl std::fmt::Display for $type {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };

    ($type:ident, $error:ident($message:literal) { $($variant:ident => $word:literal),+ $(,)? }) => {
        spelled!($type { $($variant => $word),+ });

        #[doc = concat!("A string that names no [`", stringify!($type), "`].")]
        #[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
        #[error($message)]
        pub struct $error(pub String);

        impl std::str::FromStr for $type {
            type Err = $error;

            fn from_str(input: &str) -> Result<Self, Self::Err> {
                match input.trim() {
                    $($word => Ok(Self::$variant),)+
                    other => Err($error(other.to_owned())),
                }
            }
        }
    };
}
