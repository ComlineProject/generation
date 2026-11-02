# `api`

The shop's API.

| Item | Summary |
|---|---|
| error [`OutOfStock`](#outofstock) | Not in stock. |
| struct [`Order`](#order) | One order. |
| protocol [`Orders`](#orders) | Orders things. |

<a id="outofstock"></a>

## error `OutOfStock`

Not in stock.

Ordinal `0`. Message: `{item} is out of stock`

| Field | Type |
|---|---|
| `item` | `string` |

<a id="order"></a>

## struct `Order`

One order.

| Field | Type |
|---|---|
| `customer` | [`Customer`](types.md#customer) |
| `payments` | [`Payment`](types.md#payment)`[]` |
| `method` | [`HttpMethod`](https://comlineproject.github.io/docs/std/http/#httpmethod) |
| `lines` | union(`u32` `string`) |
| `greeting` | [`Greeting`](deps/localdeps/greeting.md#greeting) (optional) |

<a id="orders"></a>

## protocol `Orders`

Orders things.

<a id="orders-place"></a>

### function `place`

```comline
function place(order: Order) -> u64 ! OutOfStock;
```

Place an order.

| Argument | Type | Description |
|---|---|---|
| `order` | [`Order`](#order) | what to place |

<a id="orders-ping"></a>

### function `ping`

```comline
function ping();
```

**On the wire**

| | | from |
|---|---|---|
| format | `packed@1` | core's default |
| framing | `datagram@2` | core's default |
| handshake | `comline@1` | core's default |

**Guarantees**

| guarantee | | why | accepted |
|---|---|---|---|
| schema agreement | held |  |  |
| encoding agreement | held |  |  |
| encoding conformance | held |  |  |
| version honesty | held |  |  |
