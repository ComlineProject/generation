# `types`

Shared types.

| Item | Summary | Value |
|---|---|---|
| struct [`Customer`](#customer) | Who buys. |  |
| enum [`Payment`](#payment) | How an order is paid. |  |
| const [`MAX_NAME`](#max_name) | The longest name. | `u32 = 64` |
| type [`CustomerId`](#customerid) |  | `u64` |

<a id="customer"></a>

## struct `Customer`

Who buys.

| Field | Type | Description |
|---|---|---|
| `id` | `u64` | The customer's id. |
| `name` | `string` | `@deprecated = "use email"` |
| `email` | `string` (optional) |  |
| `phone` | `string` (optional) |  |

<a id="payment"></a>

## enum `Payment`

How an order is paid.

Variants:

- `Card`

<a id="max_name"></a>

## const `MAX_NAME`

The longest name.

```comline
const MAX_NAME: u32 = 64
```

<a id="customerid"></a>

## type `CustomerId`

Alias of `u64`
