#!/usr/bin/env Rscript
# Corners looked at close: each named square of a probe's picture
# (ONYX_SHOT), 20 points from its top left at x,y in points, cut out and
# enlarged eight times with no smoothing, side by side in one strip. A
# rounded box's corner is round in it, or it is a sharp corner
# (AGENTS.md, No sharp corners; docs/probing.md, A corner, checked).
#
#   Rscript scripts/corners.R <shot.png> <strip.png> <name>=<x>,<y>...
library(png)

args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 3) stop("usage: corners.R <shot.png> <strip.png> <name>=<x>,<y>...")
img <- readPNG(args[1])
zoom <- 8
side <- 40  # 20 points, in pixels

tiles <- lapply(args[-(1:2)], function(spec) {
  xy <- as.numeric(strsplit(sub(".*=", "", spec), ",")[[1]]) * 2
  rows <- (xy[2] + 1):min(xy[2] + side, dim(img)[1])
  cols <- (xy[1] + 1):min(xy[1] + side, dim(img)[2])
  t <- img[rows, cols, 1:3, drop = FALSE]
  t[rep(seq_along(rows), each = zoom), rep(seq_along(cols), each = zoom), , drop = FALSE]
})

gap <- 16
height <- max(sapply(tiles, function(t) dim(t)[1]))
width <- sum(sapply(tiles, function(t) dim(t)[2])) + gap * (length(tiles) - 1)
strip <- array(1, c(height, width, 3))
x <- 0
for (t in tiles) {
  strip[seq_len(dim(t)[1]), x + seq_len(dim(t)[2]), ] <- t
  x <- x + dim(t)[2] + gap
}
writePNG(strip, args[2])
