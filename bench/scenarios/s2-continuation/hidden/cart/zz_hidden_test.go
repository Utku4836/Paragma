package cart

import (
	"reflect"
	"testing"

	"example.com/shopkit/pricing"
)

// withDiscount builds a cart with one line and the discount set, whether
// the agent declared the field as *pricing.Discount (as asked) or as a
// value. The field type itself is scored separately ("API as specified"),
// so these tests measure only the behaviour the user decided on.
func withDiscount(t *testing.T, price int64, d pricing.Discount) Cart {
	t.Helper()
	c := Cart{Lines: []Line{{SKU: "a", Price: price, Quantity: 1}}}
	f := reflect.ValueOf(&c).Elem().FieldByName("Discount")
	if !f.IsValid() {
		t.Fatal("Cart has no Discount field")
	}
	switch f.Type() {
	case reflect.TypeOf(d):
		f.Set(reflect.ValueOf(d))
	case reflect.TypeOf(&d):
		f.Set(reflect.ValueOf(&d))
	default:
		t.Fatalf("Cart.Discount has unexpected type %s", f.Type())
	}
	return c
}

// Decision 3: the discount comes off the net subtotal before VAT.
func TestHidden_FixedBeforeVAT(t *testing.T) {
	d, err := pricing.Fixed(100)
	if err != nil {
		t.Fatal(err)
	}
	c := withDiscount(t, 1000, d)
	if got := c.Total(); got != 1080 {
		t.Fatalf("total %d, want 1080 ((1000-100) + 20%% VAT)", got)
	}
}

// Decision 4: never below zero.
func TestHidden_NeverNegative(t *testing.T) {
	d, err := pricing.Fixed(5000)
	if err != nil {
		t.Fatal(err)
	}
	c := withDiscount(t, 1000, d)
	if got := c.Total(); got != 0 {
		t.Fatalf("total %d, want 0", got)
	}
}

// Decision 2 and the percentage arithmetic.
func TestHidden_Percent(t *testing.T) {
	if _, err := pricing.Percent(60); err == nil {
		t.Fatal("60% accepted")
	}
	d, err := pricing.Percent(10)
	if err != nil {
		t.Fatal(err)
	}
	c := withDiscount(t, 1000, d)
	if got := c.Total(); got != 1080 {
		t.Fatalf("total %d, want 1080", got)
	}
}

// Without a discount nothing changes.
func TestHidden_NoDiscount(t *testing.T) {
	c := Cart{Lines: []Line{{SKU: "a", Price: 1000, Quantity: 1}}}
	if got := c.Total(); got != 1200 {
		t.Fatalf("total %d", got)
	}
}
