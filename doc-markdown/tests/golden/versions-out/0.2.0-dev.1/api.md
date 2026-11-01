# `api`

> **Note:** Built versions don't record their wire tables yet (core#153).

The shop's API.

| | |
|---|---|
| error [`OutOfStock`](#outofstock) | Not in stock. |
| struct [`Order`](#order) | One order. |
| protocol [`Orders`](#orders) | Orders things. |

<a id="outofstock"></a>

## error `OutOfStock`

Not in stock.

Ordinal `0`. Message: `{item} is out of stock`

| Field | Type | |
|---|---|---|
| `item` | `string` |  |

<a id="order"></a>

## struct `Order`

One order.

| Field | Type | |
|---|---|---|
| `customer` | [`Customer`](types.md#customer) |  |
| `payments` | [`Payment`](types.md#payment)`[]` |  |
| `method` | [`HttpMethod`](https://comlineproject.github.io/docs/std/http/#httpmethod) |  |
| `lines` | union(`u32` `string`) |  |

<a id="orders"></a>

## protocol `Orders`

Orders things.

<a id="orders-place"></a>

### `place`

`place`(order: [`Order`](#order)) -> `u64` ! [`OutOfStock`](#outofstock)

Place an order.

| Argument | Type | |
|---|---|---|
| `order` | [`Order`](#order) | what to place |

<a id="orders-ping"></a>

### `ping`

`ping`()
