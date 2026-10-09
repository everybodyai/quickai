//! The context expressions see (docs/architecture.md, section 7.1), as a schema for the type
//! checker and as the value the planner fills in.
//!
//! One definition serves both: the schema says what each name is, the planner supplies the
//! values, and a test keeps the two in step.

use crate::doc::engine::{EngineDoc, FieldType};
use crate::expr::types::Type;

/// The type a field's value has inside expressions.
pub fn field_value_type(ty: FieldType) -> Type {
    match ty {
        FieldType::Int | FieldType::Port => Type::Int,
        FieldType::Float => Type::Float,
        FieldType::Bool => Type::Bool,
        FieldType::Text
        | FieldType::Choice
        | FieldType::Dir
        | FieldType::File
        | FieldType::Url
        | FieldType::RuntimePath
        | FieldType::KvLines
        | FieldType::ModelRef
        | FieldType::Hidden => Type::Str,
    }
}

/// What a use site of an expression or template may see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    /// Anywhere outside a field's own bindings.
    Engine,
    /// Inside a field's bindings, where `value` is that field's current value.
    Field(FieldType),
}

/// The schema of the context for `engine`'s documents.
pub fn schema(engine: &EngineDoc, site: Site) -> Type {
    let fields = Type::Map(
        engine
            .fields
            .iter()
            .map(|f| (f.id.clone(), field_value_type(f.ty)))
            .collect(),
    );
    let mut root = match Type::object([
        ("os", Type::Str),
        ("arch", Type::Str),
        (
            "sys",
            Type::object([("ram_bytes", Type::Int), ("cpus", Type::Int)]),
        ),
        (
            "gpu",
            Type::object([
                ("known", Type::Bool),
                ("vendors", Type::list_of(Type::Str)),
                ("targets", Type::list_of(Type::Str)),
            ]),
        ),
        (
            "runtime",
            Type::object([
                ("binary", Type::Str),
                ("dir", Type::Str),
                ("build", Type::Int),
                ("version", Type::Str),
                ("flags", Type::list_of(Type::Str)),
            ]),
        ),
        (
            "model",
            Type::object([
                ("id", Type::Str),
                ("ref", Type::Str),
                ("display", Type::Str),
                ("size_bytes", Type::Int),
                (
                    "modalities",
                    Type::object([
                        ("input", Type::list_of(Type::Str)),
                        ("output", Type::list_of(Type::Str)),
                    ]),
                ),
                ("main", Type::object([("path", Type::Str)])),
                ("components", Type::Any),
            ]),
        ),
        ("listen_port", Type::Int),
        ("public_port", Type::Int),
        (
            "session",
            Type::object([
                ("running", Type::Bool),
                ("crashed", Type::Bool),
                ("attached", Type::Bool),
                ("model_display", Type::Str),
                ("model_ref", Type::Str),
            ]),
        ),
        ("app", Type::Any),
        ("data", Type::Str),
        ("config", Type::Str),
        ("exe_dir", Type::Str),
        ("home", Type::Str),
    ]) {
        Type::Map(map) => map,
        _ => unreachable!("object() builds a map"),
    };
    root.insert("field".to_string(), fields);
    if let Site::Field(ty) = site {
        root.insert("value".to_string(), field_value_type(ty));
    }
    Type::Map(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(fields: &str) -> EngineDoc {
        toml::from_str(&format!("[engine]\nname = 'x'\n{fields}")).unwrap()
    }

    #[test]
    fn fields_appear_under_field_with_their_types() {
        let doc = engine(
            "[[field]]\nid='port'\ntype='port'\n[[field]]\nid='extra_args'\ntype='text'\n[[field]]\nid='flag'\ntype='bool'\n",
        );
        let s = schema(&doc, Site::Engine);
        assert_eq!(s.resolve(&["field".into(), "port".into()]), Ok(Type::Int));
        assert_eq!(
            s.resolve(&["field".into(), "extra_args".into()]),
            Ok(Type::Str)
        );
        assert_eq!(s.resolve(&["field".into(), "flag".into()]), Ok(Type::Bool));
        assert!(s.resolve(&["field".into(), "nope".into()]).is_err());
    }

    #[test]
    fn value_exists_only_inside_a_field() {
        let doc = engine("");
        assert!(schema(&doc, Site::Engine)
            .resolve(&["value".into()])
            .is_err());
        assert_eq!(
            schema(&doc, Site::Field(FieldType::Int)).resolve(&["value".into()]),
            Ok(Type::Int)
        );
        assert_eq!(
            schema(&doc, Site::Field(FieldType::Text)).resolve(&["value".into()]),
            Ok(Type::Str)
        );
    }

    #[test]
    fn the_documented_names_are_all_present() {
        let s = schema(&engine(""), Site::Engine);
        for name in [
            "os",
            "arch",
            "sys.ram_bytes",
            "sys.cpus",
            "gpu.known",
            "gpu.vendors",
            "runtime.binary",
            "runtime.dir",
            "runtime.build",
            "runtime.version",
            "runtime.flags",
            "model.id",
            "model.ref",
            "model.size_bytes",
            "model.modalities.input",
            "model.main.path",
            "model.components",
            "listen_port",
            "public_port",
            "session.running",
            "session.crashed",
            "app",
            "data",
            "config",
            "exe_dir",
            "home",
        ] {
            let path: Vec<String> = name.split('.').map(str::to_string).collect();
            assert!(
                s.resolve(&path).is_ok(),
                "{name} is missing from the schema"
            );
        }
    }
}
