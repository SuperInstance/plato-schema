# plato-schema

> JSON schema validation for PLATO tile data and configuration

## What This Does

plato-schema validates JSON data against schemas defined in code. It checks types, ranges, required fields, string patterns, and enum values — ensuring tile data, sensor configurations, and pipeline settings conform to expected shapes before processing.

## The Key Idea

Bad data causes subtle bugs. A temperature that's suddenly -999 or a sensor_id that's empty can cascade through the entire pipeline. plato-schema catches these problems at the boundary: validate once when data enters the system, then trust it downstream.

## Install

```bash
cargo add plato-schema
```

## Quick Start

```rust
use plato_schema::*;

// Define a schema for temperature readings
let mut schema = Schema::new();
schema.add_field(SchemaField::new("temperature")
    .type_of(SchemaType::Number)
    .range(-50.0, 150.0)
    .required());
schema.add_field(SchemaField::new("sensor_id")
    .type_of(SchemaType::String)
    .required());

// Validate data
let result = schema.validate(&data);
if !result.valid {
    for err in &result.errors {
        println!("{}: {}", err.field, err.message);
    }
}
```

## Testing

27 tests covering field definitions, type checking, range validation, required fields, pattern matching, enum validation, nested schemas, and error reporting.

## License

Apache-2.0
