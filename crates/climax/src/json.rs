// SPDX-License-Identifier: EUPL-1.2

//! json: `serde_json` with non-finite floats written as strings

use core::fmt;

use serde::{
    Serialize,
    Serializer,
    ser::{
        SerializeMap,
        SerializeSeq,
        SerializeStruct,
        SerializeStructVariant,
        SerializeTuple,
        SerializeTupleStruct,
        SerializeTupleVariant,
    },
};

/// `value` as one line of JSON, where NaN and the infinities become the strings
/// "NaN", "Infinity" and "-Infinity" instead of the `null` of `serde_json`
pub(crate) fn to_line<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = Vec::new();
    value.serialize(Adapter(&mut serde_json::Serializer::new(&mut bytes)))?;
    bytes.push(b'\n');
    Ok(bytes)
}

const fn non_finite(value: f64) -> Option<&'static str> {
    if value.is_finite() {
        None
    } else if value.is_nan() {
        Some("NaN")
    } else if value.is_sign_positive() {
        Some("Infinity")
    } else {
        Some("-Infinity")
    }
}

/// forwards to the inner serializer and wraps every nested value the same way
struct Adapter<S>(S);

struct Nested<'a, T: ?Sized>(&'a T);

impl<T: Serialize + ?Sized> Serialize for Nested<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(Adapter(serializer))
    }
}

macro_rules! forward {
    ($($method:ident($ty:ty)),* $(,)?) => {
        $(
            fn $method(self, value: $ty) -> Result<S::Ok, S::Error> {
                self.0.$method(value)
            }
        )*
    };
}

impl<S: Serializer> Serializer for Adapter<S> {
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = Adapter<S::SerializeSeq>;
    type SerializeTuple = Adapter<S::SerializeTuple>;
    type SerializeTupleStruct = Adapter<S::SerializeTupleStruct>;
    type SerializeTupleVariant = Adapter<S::SerializeTupleVariant>;
    type SerializeMap = Adapter<S::SerializeMap>;
    type SerializeStruct = Adapter<S::SerializeStruct>;
    type SerializeStructVariant = Adapter<S::SerializeStructVariant>;

    forward! {
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_i128(i128),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_u128(u128),
        serialize_char(char),
        serialize_str(&str),
        serialize_bytes(&[u8]),
        serialize_unit_struct(&'static str),
    }

    fn serialize_f32(self, v: f32) -> Result<S::Ok, S::Error> {
        match non_finite(v.into()) {
            Some(text) => self.0.serialize_str(text),
            None => self.0.serialize_f32(v),
        }
    }

    fn serialize_f64(self, v: f64) -> Result<S::Ok, S::Error> {
        match non_finite(v) {
            Some(text) => self.0.serialize_str(text),
            None => self.0.serialize_f64(v),
        }
    }

    fn serialize_none(self) -> Result<S::Ok, S::Error> {
        self.0.serialize_none()
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<S::Ok, S::Error> {
        self.0.serialize_some(&Nested(value))
    }

    fn serialize_unit(self) -> Result<S::Ok, S::Error> {
        self.0.serialize_unit()
    }

    fn serialize_unit_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
    ) -> Result<S::Ok, S::Error> {
        self.0.serialize_unit_variant(name, variant_index, variant)
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<S::Ok, S::Error> {
        self.0.serialize_newtype_struct(name, &Nested(value))
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<S::Ok, S::Error> {
        self.0
            .serialize_newtype_variant(name, variant_index, variant, &Nested(value))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, S::Error> {
        self.0.serialize_seq(len).map(Adapter)
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, S::Error> {
        self.0.serialize_tuple(len).map(Adapter)
    }

    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, S::Error> {
        self.0.serialize_tuple_struct(name, len).map(Adapter)
    }

    fn serialize_tuple_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, S::Error> {
        self.0
            .serialize_tuple_variant(name, variant_index, variant, len)
            .map(Adapter)
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, S::Error> {
        self.0.serialize_map(len).map(Adapter)
    }

    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, S::Error> {
        self.0.serialize_struct(name, len).map(Adapter)
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, S::Error> {
        self.0
            .serialize_struct_variant(name, variant_index, variant, len)
            .map(Adapter)
    }

    fn collect_str<T: fmt::Display + ?Sized>(self, value: &T) -> Result<S::Ok, S::Error> {
        self.0.collect_str(value)
    }

    fn is_human_readable(&self) -> bool {
        self.0.is_human_readable()
    }
}

impl<C: SerializeSeq> SerializeSeq for Adapter<C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), C::Error> {
        self.0.serialize_element(&Nested(value))
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        self.0.end()
    }
}

impl<C: SerializeTuple> SerializeTuple for Adapter<C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), C::Error> {
        self.0.serialize_element(&Nested(value))
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        self.0.end()
    }
}

impl<C: SerializeTupleStruct> SerializeTupleStruct for Adapter<C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), C::Error> {
        self.0.serialize_field(&Nested(value))
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        self.0.end()
    }
}

impl<C: SerializeTupleVariant> SerializeTupleVariant for Adapter<C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), C::Error> {
        self.0.serialize_field(&Nested(value))
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        self.0.end()
    }
}

impl<C: SerializeMap> SerializeMap for Adapter<C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), C::Error> {
        self.0.serialize_key(&Nested(key))
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), C::Error> {
        self.0.serialize_value(&Nested(value))
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        self.0.end()
    }
}

impl<C: SerializeStruct> SerializeStruct for Adapter<C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), C::Error> {
        self.0.serialize_field(key, &Nested(value))
    }

    fn skip_field(&mut self, key: &'static str) -> Result<(), C::Error> {
        self.0.skip_field(key)
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        self.0.end()
    }
}

impl<C: SerializeStructVariant> SerializeStructVariant for Adapter<C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), C::Error> {
        self.0.serialize_field(key, &Nested(value))
    }

    fn skip_field(&mut self, key: &'static str) -> Result<(), C::Error> {
        self.0.skip_field(key)
    }

    fn end(self) -> Result<C::Ok, C::Error> {
        self.0.end()
    }
}

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeMap;

    use super::*;

    #[derive(Serialize)]
    struct Reading {
        value:   f64,
        samples: Vec<f32>,
        pair:    (f64, Option<f64>),
    }

    #[derive(Serialize)]
    enum Shape {
        Point(f64),
        Line { length: f64 },
    }

    fn line<T: Serialize + ?Sized>(value: &T) -> String {
        String::from_utf8(to_line(value).unwrap()).unwrap()
    }

    #[test]
    fn non_finite_floats_are_strings_at_any_depth() {
        let reading = Reading {
            value:   f64::NAN,
            samples: vec![f32::INFINITY, 1.5, f32::NEG_INFINITY],
            pair:    (f64::NEG_INFINITY, Some(f64::NAN)),
        };
        assert_eq!(
            line(&reading),
            "{\"value\":\"NaN\",\"samples\":[\"Infinity\",1.5,\"-Infinity\"],\"pair\":[\"\
             -Infinity\",\"NaN\"]}\n"
        );
        assert_eq!(
            line(&Shape::Point(f64::INFINITY)),
            "{\"Point\":\"Infinity\"}\n"
        );
        assert_eq!(
            line(&Shape::Line { length: 2.0 }),
            "{\"Line\":{\"length\":2.0}}\n"
        );
        assert_eq!(
            line(&BTreeMap::from([("k", f64::NAN)])),
            "{\"k\":\"NaN\"}\n"
        );
    }
}
