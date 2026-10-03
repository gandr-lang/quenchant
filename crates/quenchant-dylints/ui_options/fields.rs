type Missing<T> = Option<T>;

#[repr(transparent)]
#[derive(Clone, Copy)]
struct Value(u8);

// Plain items: every Option reached by a field is reported, through aliases,
// containers, references, and tuples, and in enum variants.
struct Plain {
    direct: Option<Value>,
    aliased: Missing<Value>,
    nested: Vec<(Value, Option<Value>)>,
    borrowed: &'static Option<Value>,
    kept: Value,
}

enum Choice {
    Some { value: Option<Value> },
    Tuple(Missing<Value>),
    Unit,
}

// Wire form by derive: a serialization target keeps its Option fields.
#[derive(serde::Serialize)]
struct Serialized {
    direct: Option<Value>,
    nested: Vec<Option<Value>>,
}

#[derive(serde::Deserialize)]
enum Parsed {
    Some { value: Option<String> },
}

// Wire form by hand: the implementation index, not the attribute, decides.
#[repr(transparent)]
struct Written {
    direct: Option<Value>,
}
impl serde::Serialize for Written {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bool(self.direct.is_some())
    }
}

// Wire form by argument parsing: every clap derive implements FromArgMatches.
#[derive(clap::Parser)]
#[repr(transparent)]
struct Arguments {
    #[arg(long)]
    name: Option<String>,
}

// A derive on a neighbour exempts nothing here.
#[repr(transparent)]
struct Beside {
    direct: Option<Value>,
}

impl serde::Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(self.0)
    }
}

fn main() {}
