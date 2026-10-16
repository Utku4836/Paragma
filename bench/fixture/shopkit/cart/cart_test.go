package cart

import "testing"

func TestTotal(t *testing.T) {
	c := Cart{Lines: []Line{{SKU: "a", Price: 1000, Quantity: 2}, {SKU: "b", Price: 500, Quantity: 1}}}
	if got := c.Subtotal(); got != 2500 {
		t.Fatalf("subtotal %d", got)
	}
	if got := c.Total(); got != 3000 {
		t.Fatalf("total %d", got)
	}
}
