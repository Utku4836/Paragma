package cart

import (
	"testing"

	"example.com/shopkit/pricing"
)

func one(price int64) []Line { return []Line{{SKU: "a", Price: price, Quantity: 1}} }

// Decision 3: the discount comes off the net subtotal before VAT.
func TestHidden_FixedBeforeVAT(t *testing.T) {
	d, err := pricing.Fixed(100)
	if err != nil {
		t.Fatal(err)
	}
	c := Cart{Lines: one(1000), Discount: &d}
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
	c := Cart{Lines: one(1000), Discount: &d}
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
	c := Cart{Lines: one(1000), Discount: &d}
	if got := c.Total(); got != 1080 {
		t.Fatalf("total %d, want 1080", got)
	}
}

// Without a discount nothing changes.
func TestHidden_NoDiscount(t *testing.T) {
	c := Cart{Lines: one(1000)}
	if got := c.Total(); got != 1200 {
		t.Fatalf("total %d", got)
	}
}
