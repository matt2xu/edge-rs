Version 0.1.0 (2016-xx-xx)
==========================


* The `#[deprecated]` attribute when applied to an API will generate
  warnings when used. The warnings may be suppressed with
  `#[allow(deprecated)]`. [RFC 1270].
* [`fn` item types are zero sized, and each `fn` names a unique
  type][1.9fn]. This will break code that transmutes `fn`s, so calling
  `transmute` on a `fn` type will generate a warning for a few cycles,
  then will be converted to an error.
* [Field and method resolution understand visibility, so private
  fields and methods cannot prevent the proper use of public fields
  and methods][1.9fv].
* [The parser considers unicode codepoints in the
  `PATTERN_WHITE_SPACE` category to be whitespace][1.9ws].

Stabilized APIs
---------------

* [`std::panic`]
* [`std::panic::catch_unwind`][] (renamed from `recover`)
* [`std::panic::resume_unwind`][] (renamed from `propagate`)
* [`std::panic::AssertUnwindSafe`][] (renamed from `AssertRecoverSafe`)
* [`std::panic::UnwindSafe`][] (renamed from `RecoverSafe`)
* [`str::is_char_boundary`]
* [`<*const T>::as_ref`]
