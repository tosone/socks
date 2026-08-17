// Copyright 2014 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style
// license that can be found in the LICENSE file.

//go:build !android

package main

import (
	"fmt"
	"log/slog"
)

func logInit() {
}

func logFatal(v ...any) {
	slog.Error(fmt.Sprint(v...))
}

func logWarn(v ...any) {
	slog.Warn(fmt.Sprint(v...))
}

func logInfo(v ...any) {
	slog.Info(fmt.Sprint(v...))
}
