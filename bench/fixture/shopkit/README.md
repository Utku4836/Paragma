# shopkit

Small shop backend library: catalog, cart, pricing, payments.

## Packages

- `catalog`: loads the product catalog and verifies its checksum
- `cart`: cart lines and totals
- `pricing`: tax and price helpers
- `payments`: charging customers (amounts in kuruş, int64)
- `audit`: audit log of money movements
- `clock`: replaceable time source for tests

## Development

```sh
go test ./...
```
