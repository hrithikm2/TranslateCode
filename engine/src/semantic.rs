//! Language-neutral type normalization and lightweight expression inference.
//!
//! Frontends may spell the same type in very different ways (`List<int>`,
//! `list[int]`, `[]int`, `[Int]`, or `Vec<i64>`).  Keeping that knowledge in
//! emitters caused dynamic-to-static translations to degrade to `Object`/`any`.
//! This module is the single normalization boundary used before emission.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticType {
    String,
    Integer,
    Float,
    Boolean,
    Void,
    Any,
    List(Box<SemanticType>),
    Set(Box<SemanticType>),
    Map(Box<SemanticType>, Box<SemanticType>),
}

impl SemanticType {
    pub fn parse(annotation: &str) -> Option<Self> {
        let mut value = annotation
            .trim()
            .trim_start_matches('&')
            .trim_end_matches('?')
            .trim();
        if let Some(rest) = value.strip_prefix("mut ") {
            value = rest.trim();
        }

        if let Some(element) = value.strip_prefix("[]") {
            return Self::parse(element).map(|item| Self::List(Box::new(item)));
        }
        if value.starts_with('[') && value.ends_with(']') {
            let inner = &value[1..value.len() - 1];
            if let Some((key, item)) = split_once_top_level(inner, ':') {
                return Some(Self::Map(
                    Box::new(Self::parse(key)?),
                    Box::new(Self::parse(item)?),
                ));
            }
            return Self::parse(inner).map(|item| Self::List(Box::new(item)));
        }

        for (open, close) in [('<', '>'), ('[', ']')] {
            let Some(index) = value.find(open) else {
                continue;
            };
            if !value.ends_with(close) {
                continue;
            }
            let container = value[..index].trim();
            let arguments = split_top_level(&value[index + 1..value.len() - 1], ',')
                .into_iter()
                .map(Self::parse)
                .collect::<Option<Vec<_>>>()?;
            return match container {
                "List" | "Array" | "Vec" | "Iterable" | "list" | "tuple" => arguments
                    .into_iter()
                    .next()
                    .map(|item| Self::List(Box::new(item))),
                "Set" | "HashSet" | "set" => arguments
                    .into_iter()
                    .next()
                    .map(|item| Self::Set(Box::new(item))),
                "Map" | "HashMap" | "dict" if arguments.len() == 2 => Some(Self::Map(
                    Box::new(arguments[0].clone()),
                    Box::new(arguments[1].clone()),
                )),
                _ => None,
            };
        }

        Some(match value {
            "str" | "String" | "string" => Self::String,
            "int" | "Int" | "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32"
            | "u64" | "usize" => Self::Integer,
            "float" | "double" | "Float" | "Double" | "f32" | "f64" | "num" => Self::Float,
            "bool" | "boolean" | "Boolean" | "Bool" => Self::Boolean,
            "void" | "Void" | "()" | "None" => Self::Void,
            "dynamic" | "Object" | "Any" | "any" | "interface{}" => Self::Any,
            _ => return None,
        })
    }

    pub fn infer(expression: &str) -> Self {
        let value = expression.trim();
        if matches!(value, "true" | "false" | "True" | "False") {
            return Self::Boolean;
        }
        if value.starts_with('"') || value.starts_with('\'') || value.contains("format!(") {
            return Self::String;
        }
        if value.parse::<i64>().is_ok() {
            return Self::Integer;
        }
        if value.parse::<f64>().is_ok() {
            return Self::Float;
        }
        if value.starts_with('[') && value.ends_with(']') {
            let inner = &value[1..value.len() - 1];
            let element = split_top_level(inner, ',')
                .into_iter()
                .find(|item| !item.trim().is_empty())
                .map(Self::infer)
                .unwrap_or(Self::Any);
            return Self::List(Box::new(element));
        }
        if [" + ", " - ", " * ", " / ", " % "]
            .iter()
            .any(|operator| value.contains(operator))
        {
            return Self::Integer;
        }
        Self::Any
    }

    pub fn canonical(&self) -> String {
        match self {
            Self::String => "string".into(),
            Self::Integer => "int".into(),
            Self::Float => "float".into(),
            Self::Boolean => "bool".into(),
            Self::Void => "void".into(),
            Self::Any => "any".into(),
            Self::List(value) => format!("list<{}>", value.canonical()),
            Self::Set(value) => format!("set<{}>", value.canonical()),
            Self::Map(key, value) => format!("map<{},{}>", key.canonical(), value.canonical()),
        }
    }
}

fn split_once_top_level(value: &str, separator: char) -> Option<(&str, &str)> {
    let mut depth = 0i32;
    for (index, character) in value.char_indices() {
        match character {
            '<' | '[' | '(' | '{' => depth += 1,
            '>' | ']' | ')' | '}' => depth -= 1,
            current if current == separator && depth == 0 => {
                return Some((&value[..index], &value[index + current.len_utf8()..]));
            }
            _ => {}
        }
    }
    None
}

fn split_top_level(value: &str, separator: char) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    for (index, character) in value.char_indices() {
        match character {
            '<' | '[' | '(' | '{' => depth += 1,
            '>' | ']' | ')' | '}' => depth -= 1,
            current if current == separator && depth == 0 => {
                result.push(value[start..index].trim());
                start = index + current.len_utf8();
            }
            _ => {}
        }
    }
    result.push(value[start..].trim());
    result
}

#[cfg(test)]
mod tests {
    use super::SemanticType;

    #[test]
    fn normalizes_collection_types_from_every_supported_family() {
        for annotation in ["List<int>", "list[int]", "[]int", "[Int]", "Vec<i64>"] {
            assert_eq!(
                SemanticType::parse(annotation).unwrap().canonical(),
                "list<int>",
                "failed to normalize {annotation}"
            );
        }
        assert_eq!(
            SemanticType::parse("dict[str, list[int]]")
                .unwrap()
                .canonical(),
            "map<string,list<int>>"
        );
    }
}
