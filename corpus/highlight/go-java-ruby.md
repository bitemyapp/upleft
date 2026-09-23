# Go, Java, Ruby

```go
package main

import (
	"fmt"
	"strings"
)

const MaxSize = 1 << 10

type Stack[T any] struct{ items []T }

func (s *Stack[T]) Push(v T) { s.items = append(s.items, v) }

func main() {
	raw := `raw string with \n and "quotes"
spanning lines`
	r := 'x'
	ch := make(chan int, 3)
	go func() { ch <- iota }()
	select {
	case v := <-ch:
		fmt.Println(v, nil, true, 0x1p-2, 1_000, 07)
	default:
	}
	defer strings.TrimSpace(" x ")
}
```

```java
package com.example;

import java.util.*;

@FunctionalInterface
public sealed interface Shape permits Circle, Square {
    double area();
}

public final class Main {
    private static final int MAX_COUNT = 100;
    public static void main(String[] args) throws Exception {
        var list = new ArrayList<String>();
        String text = """
            A text block with "quotes"
            """;
        char c = 'c'; long l = 10L; double d = 1.5e10d; float f = .5f;
        record Point(int x, int y) {}
        if (args.length > 0 && args[0] instanceof String s) { System.out.println(s + this); }
        synchronized (Main.class) { yield null; }
    }
}
```

```ruby
# frozen_string_literal: true
require 'json'
require_relative "lib/helper"

module Greeting
  VERSION = "1.0.0"
  class Person < Struct
    attr_accessor :name, :age
    @@count = 0
    def initialize(name, age = nil)
      @name = name; @age = age; $global = true
      @@count += 1
      puts "Hello, #{name}!" unless name.nil?
      raise ArgumentError, 'bad' if age && age < 0
    end

    def to_h = { name: @name, "age" => @age, :sym => Foo::Bar }
  end
end
=begin
a block comment at column zero
=end
  =begin not a comment when indented
x = [1, 2.5, 0x1F, 1_000].map { |v| v * 2 }.select(&:even?)
str = 'single quoted
spanning lines'
```

```rb
lambda { |x| x }.call(1); proc do yield end; __FILE__; __LINE__; BEGIN { }; END { }
```
