# Implementation Plan - TypeScript Code Generator

Goal: Implement a TypeScript code generator (`codelib_gen/typescript.rs`) to support multi-language output.

## Proposed Changes

### 1. Create `core/src/codelib_gen/typescript.rs`
- **Function**: `pub fn generate_typescript(units: &Vec<FrozenUnit>) -> String`
- **Structs**: Map to `export interface Name { field: type; }`
- **Enums**: Map to `export enum Name { Variant = "Variant", }` (String enums for better JSON interop)
- **Protocols**: Map to `export interface Name { func(...): ret; }`
- **Type Mapping**:
    - `u8`..`u64`/`i8`..`i64`/`float`/[int](https://github.com/ComlineProject/core/blob/d8bc708bf23e4accd06a13da1d317b10596888c7/core/src/package/config/ir/interpreter/interpret.rs#L11-L29) -> [number](https://github.com/ComlineProject/core/blob/d8bc708bf23e4accd06a13da1d317b10596888c7/core/tests/schema/parser/comprehensive.rs#L233-L238)
    - `bool` -> `boolean`
    - [string](https://github.com/ComlineProject/core/blob/d8bc708bf23e4accd06a13da1d317b10596888c7/core/src/schema/idl/grammar.rs#L370-L373) -> [string](https://github.com/ComlineProject/core/blob/d8bc708bf23e4accd06a13da1d317b10596888c7/core/src/schema/idl/grammar.rs#L370-L373)
    - `Vec<T>` -> `T[]` (via name analysis)

### 2. Register in [core/src/codelib_gen/mod.rs](https://github.com/ComlineProject/core/blob/d8bc708bf23e4accd06a13da1d317b10596888c7/core/src/codelib_gen/mod.rs)
- Valid names: `"typescript"`, `"ts"`
- Map to `typescript::generate_typescript`

## Verification
- Create a unit test in `core/tests/codelib_gen.rs` (or similar) that parses a schema and generates TypeScript code.
- Verify output contains expected TS syntax.
