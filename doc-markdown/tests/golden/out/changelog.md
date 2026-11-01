# Changelog

## 0.1.0 to 0.2.0

### Breaking

#### MAJOR · [`Payment.Cash`](types.md#payment): enum variant removed

```diff
-Cash
```

#### MAJOR · [`Orders.place`](api.md#orders-place): signature changed

```diff
-function place(order: Order) -> u64 ! OutOfStock;
+function place(order: Order, rush: bool) -> u64 ! OutOfStock;
```

### Added

#### MINOR · [`Customer.phone`](types.md#customer): field added

```diff
+optional phone: string
```

### Changed

#### PATCH · [`Customer.name`](types.md#customer): @deprecated changed

```diff
+@deprecated = "use email"
```

#### PATCH · [`Order`](api.md#order): documentation changed

Its documentation changed; nothing on the wire did.
