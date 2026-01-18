use std::ffi::CString;

use derive_more::From;
use dmx::attribute::{Bool8, Color, Float, Matrix, Vector2, Vector3, Vector4};

use crate::{new::Error, strings::string_to_cstring};

#[derive(Debug, From, Clone, PartialEq)]
pub enum Attribute {
    Integer(i32),
    Float(Float),
    Bool(bool),
    String(String),
    Binary(Box<[u8]>),
    Color(Color),
    Vector2(Vector2),
    Vector3(Vector3),
    Vector4(Vector4),
    Matrix(Matrix),
    IntegerArray(Box<[i32]>),
    FloatArray(Box<[Float]>),
    BoolArray(Box<[Bool8]>),
    StringArray(Box<[String]>),
    BinaryArray(Box<[Box<[u8]>]>),
    ColorArray(Box<[Color]>),
    Vector2Array(Box<[Vector2]>),
    Vector3Array(Box<[Vector3]>),
    Vector4Array(Box<[Vector4]>),
    MatrixArray(Box<[Matrix]>),
}

impl From<f32> for Attribute {
    fn from(value: f32) -> Self {
        Self::Float(Float::from(value))
    }
}

impl Attribute {
    pub(crate) fn get_encoded_size(&self) -> usize {
        match self {
            Attribute::Integer(_) => size_of::<i32>(),
            Attribute::Float(_) => size_of::<f32>(),
            Attribute::Bool(_) => size_of::<Bool8>(),
            Attribute::String(value) => 1 + value.len(),
            Attribute::Binary(value) => size_of::<u32>() + value.len(),
            Attribute::Color(_) => size_of::<Color>(),
            Attribute::Vector2(_) => size_of::<Vector2>(),
            Attribute::Vector3(_) => size_of::<Vector3>(),
            Attribute::Vector4(_) => size_of::<Vector4>(),
            Attribute::Matrix(_) => size_of::<Matrix>(),
            Attribute::IntegerArray(value) => size_of::<u32>() + (value.len() * size_of::<i32>()),
            Attribute::FloatArray(value) => size_of::<u32>() + (value.len() * size_of::<f32>()),
            Attribute::BoolArray(value) => size_of::<u32>() + (value.len() * size_of::<Bool8>()),
            Attribute::StringArray(value) => {
                size_of::<u32>() + value.len() + value.iter().map(String::len).sum::<usize>()
            }
            Attribute::BinaryArray(value) => {
                size_of::<u32>() + value.iter().map(|value| size_of::<u32>() + value.len()).sum::<usize>()
            }
            Attribute::ColorArray(value) => size_of::<u32>() + (value.len() * size_of::<Color>()),
            Attribute::Vector2Array(value) => size_of::<u32>() + (value.len() * size_of::<Vector2>()),
            Attribute::Vector3Array(value) => size_of::<u32>() + (value.len() * size_of::<Vector3>()),
            Attribute::Vector4Array(value) => size_of::<u32>() + (value.len() * size_of::<Vector4>()),
            Attribute::MatrixArray(value) => size_of::<u32>() + (value.len() * size_of::<Matrix>()),
        }
    }
}

impl TryFrom<dmx::attribute::Attribute> for Attribute {
    type Error = Error;

    fn try_from(value: dmx::attribute::Attribute) -> Result<Self, Self::Error> {
        match value {
            dmx::attribute::Attribute::Element(_) => Err(Error::UnexpectedElementReference),
            dmx::attribute::Attribute::Integer(value) => Ok((value).into()),
            dmx::attribute::Attribute::Float(value) => Ok((value).into()),
            dmx::attribute::Attribute::Bool(value) => Ok(bool::from(value).into()),
            dmx::attribute::Attribute::String(value) => Ok(value.to_string_lossy().into_owned().into()),
            dmx::attribute::Attribute::Binary(value) => Ok(value.into()),
            dmx::attribute::Attribute::Color(value) => Ok((value).into()),
            dmx::attribute::Attribute::Vector2(value) => Ok((value).into()),
            dmx::attribute::Attribute::Vector3(value) => Ok((value).into()),
            dmx::attribute::Attribute::Vector4(value) => Ok((value).into()),
            dmx::attribute::Attribute::Matrix(value) => Ok((value).into()),
            dmx::attribute::Attribute::ElementArray(_) => Err(Error::UnexpectedElementReference),
            dmx::attribute::Attribute::IntegerArray(value) => Ok(value.into()),
            dmx::attribute::Attribute::FloatArray(value) => Ok(value.into()),
            dmx::attribute::Attribute::BoolArray(value) => Ok(value.into()),
            dmx::attribute::Attribute::StringArray(value) => Ok(value
                .into_iter()
                .map(|string| string.to_string_lossy().into_owned())
                .collect::<Box<[String]>>()
                .into()),
            dmx::attribute::Attribute::BinaryArray(value) => Ok(value.into()),
            dmx::attribute::Attribute::ColorArray(value) => Ok(value.into()),
            dmx::attribute::Attribute::Vector2Array(value) => Ok(value.into()),
            dmx::attribute::Attribute::Vector3Array(value) => Ok(value.into()),
            dmx::attribute::Attribute::Vector4Array(value) => Ok(value.into()),
            dmx::attribute::Attribute::MatrixArray(value) => Ok(value.into()),
        }
    }
}

impl From<Attribute> for dmx::attribute::Attribute {
    fn from(value: Attribute) -> Self {
        match value {
            Attribute::Integer(value) => value.into(),
            Attribute::Float(value) => value.into(),
            Attribute::Bool(value) => value.into(),
            Attribute::String(value) => string_to_cstring(value).into(),
            Attribute::Binary(value) => value.into(),
            Attribute::Color(value) => value.into(),
            Attribute::Vector2(value) => value.into(),
            Attribute::Vector3(value) => value.into(),
            Attribute::Vector4(value) => value.into(),
            Attribute::Matrix(value) => value.into(),
            Attribute::IntegerArray(value) => value.into(),
            Attribute::FloatArray(value) => value.into(),
            Attribute::BoolArray(value) => value.into(),
            Attribute::StringArray(value) => value
                .into_iter()
                .map(string_to_cstring)
                .collect::<Box<[CString]>>()
                .into(),
            Attribute::BinaryArray(value) => value.into(),
            Attribute::ColorArray(value) => value.into(),
            Attribute::Vector2Array(value) => value.into(),
            Attribute::Vector3Array(value) => value.into(),
            Attribute::Vector4Array(value) => value.into(),
            Attribute::MatrixArray(value) => value.into(),
        }
    }
}
