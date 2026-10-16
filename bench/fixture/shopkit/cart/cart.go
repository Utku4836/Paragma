// Package cart models a shopping cart.
package cart

import "example.com/shopkit/pricing"

// Line is one product in the cart.
type Line struct {
	SKU      string
	Price    int64 // net unit price, kuruş
	Quantity int
}

// Cart is a list of lines.
type Cart struct {
	Lines []Line
}

// Subtotal is the net sum of all lines.
func (c *Cart) Subtotal() int64 {
	var s int64
	for _, l := range c.Lines {
		s += l.Price * int64(l.Quantity)
	}
	return s
}

// Total is the amount to charge: subtotal plus VAT.
func (c *Cart) Total() int64 {
	return pricing.WithTax(c.Subtotal())
}
