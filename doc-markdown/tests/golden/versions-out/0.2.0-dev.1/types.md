# `types`

> **Note:** Built versions don't record their wire tables yet (core#153).

Shared types.

| Item | Summary |
|---|---|
| struct [`Customer`](#customer) | Who buys. |
| enum [`Payment`](#payment) | How an order is paid. |
| const [`MAX_NAME`](#max_name) | The longest name. |
| type [`CustomerId`](#customerid) |  |

<a id="customer"></a>

## struct `Customer`

Who buys.

| Field | Type | |
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

Type: `u32`. Value: `64`

<a id="customerid"></a>

## type `CustomerId`

Alias of `u64`
