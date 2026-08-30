# Implementation Plan - TypeScript Code Generator

Goal: Implement a TypeScript code generator (`codelib_gen/typescript.rs`) to support multi-language output.

## Proposed Changes

### 1. Create `core/src/codelib_gen/typescript.rs`
- **Function**: `pub fn generate_typescript(units: &Vec<FrozenUnit>) -> String`
- **Structs**: Map to `export interface Name { field: type; }`
- **Enums**: Map to `export enum Name { Variant = "Variant", }` (String enums for better JSON interop)
- **Protocols**: Map to `export interface Name { func(...): ret; }`
- **Type Mapping**:
    - `u8`..`u64`/`i8`..`i64`/`float`/[int](file:///home/ag/Documents/shared/projects/GM%20-%20Dev/comline/comline-rs/core/core/src/package/config/ir/interpreter/interpret.rs#11-29) -> [number](file:///home/ag/Documents/shared/projects/GM%20-%20Dev/comline/comline-rs/core/core/tests/schema/parser/comprehensive.rs#233-238)
    - `bool` -> `boolean`
    - [string](file:///home/ag/Documents/shared/projects/GM%20-%20Dev/comline/comline-rs/core/core/src/schema/idl/grammar.rs#370-373) -> [string](file:///home/ag/Documents/shared/projects/GM%20-%20Dev/comline/comline-rs/core/core/src/schema/idl/grammar.rs#370-373)
    - `Vec<T>` -> `T[]` (via name analysis)

### 2. Register in [core/src/codelib_gen/mod.rs](file:///home/ag/Documents/shared/projects/GM%20-%20Dev/comline/comline-rs/core/core/src/codelib_gen/mod.rs)
- Valid names: `"typescript"`, `"ts"`
- Map to `typescript::generate_typescript`

## Verification
- Create a unit test in `core/tests/codelib_gen.rs` (or similar) that parses a schema and generates TypeScript code.
- Verify output contains expected TS syntax.
