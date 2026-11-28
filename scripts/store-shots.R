#!/usr/bin/env Rscript
# The App Store's pictures of Onyx on an iPhone and an iPad: the simulator's
# build started on the sample vault (crates/onyx-app/ios/store/vault) in two
# simulators of its own, at the sizes the store asks for (an iPhone of 6.9
# inches, 1320 by 2868, and an iPad of 13, 2064 by 2752), each state reached
# with the probe's steps (docs/probing.md) and pictured from outside. They
# land in dist/store/<iphone|ipad>/, where scripts/store-ios.R finds them.
#
#   cargo build -p onyx-app --target aarch64-apple-ios-sim
#   Rscript scripts/store-shots.R [a picture's name, as 2-drawer ...]
#
# A step's place is in the window's points, so it holds for these two
# models alone; a change to the sidebar's rows or to the sample vault's
# folders moves the taps below.

args <- commandArgs(FALSE)
setwd(file.path(dirname(normalizePath(sub("^--file=", "", grep("^--file=", args, value = TRUE)))), ".."))
only <- commandArgs(TRUE)

# The simulator's name, and the model it is made from when it is not there.
sims <- list(iphone = c("Onyx Shots Phone", "iPhone 17 Pro Max"), ipad = c("Onyx Shots Pad", "iPad Pro 13-inch (M5)"))

# Each picture: the device, its name, the appearance, the note, the steps,
# and the settings' section when the settings are what it shows. On an
# iPhone the drawer is out at a launch, and `sidebar` puts it away.
kyoto <- "Travel/Kyoto.md"
shots <- list(
  list("iphone", "1-note", "light", kyoto, "sidebar"),
  list("iphone", "2-drawer", "light", kyoto, "wait=800;tap=98,469;wait=500;tap=98,349;wait=500;tap=98,309;wait=600;tap=120,709;wait=1200;tap=26,88"),
  list("iphone", "3-dark", "dark", "Projects/Website.md", "sidebar;wait=800;pan=220,600,220,380"),
  list("iphone", "4-writing", "light", "Recipes/Sourdough.md", "sidebar;wait=800;tap=40,226"),
  list("iphone", "5-settings", "light", kyoto, "sidebar", "general"),
  list("ipad", "1-note", "light", "Recipes/Sourdough.md", "wait=800;tap=83,337;wait=500;tap=83,247;wait=600;tap=100,337;wait=900;tap=100,457"),
  list("ipad", "2-source", "dark", "Projects/Website.md", "source;wait=800;tap=83,247;wait=600;tap=100,337"),
  list("ipad", "3-settings", "light", kyoto, "wait=500", "general")
)

simctl <- function(..., quiet = TRUE) {
  system2("xcrun", c("simctl", shQuote(c(...))), stdout = if (quiet) FALSE else "", stderr = if (quiet) FALSE else "")
}

# The simulator made if it is not there, in English whatever the Mac speaks,
# and booted.
ready <- function(sim) {
  listed <- system2("xcrun", c("simctl", "list", "devices"), stdout = TRUE)
  if (!any(grepl(sim[1], listed, fixed = TRUE))) {
    runtimes <- system2("xcrun", c("simctl", "list", "runtimes"), stdout = TRUE)
    runtime <- sub(".* - ", "", tail(grep("^iOS ", runtimes, value = TRUE), 1))
    if (simctl("create", sim[1], sim[2], runtime) != 0) stop("store-shots: no simulator made from ", sim[2], " (xcrun simctl list devicetypes)")
    simctl("bootstatus", sim[1], "-b")
    simctl("spawn", sim[1], "defaults", "write", "Apple Global Domain", "AppleLanguages", "-array", "en-US")
    simctl("spawn", sim[1], "defaults", "write", "Apple Global Domain", "AppleLocale", "-string", "en_US")
    simctl("spawn", sim[1], "defaults", "write", "Apple Global Domain", "AppleKeyboards", "-array", "en_US@sw=QWERTY;hw=Automatic")
    simctl("shutdown", sim[1])
  }
  simctl("bootstatus", sim[1], "-b")
  simctl("status_bar", sim[1], "override", "--time", "9:41", "--batteryState", "charged", "--batteryLevel", "100", "--wifiBars", "3", "--cellularBars", "4", "--dataNetwork", "wifi")
}

vault <- "crates/onyx-app/ios/store/vault/Notebook"
scratch <- tempfile("onyx-store-")
dir.create(scratch)
used <- character()
for (shot in shots) {
  if (length(only) && !shot[[2]] %in% only) next
  sim <- sims[[shot[[1]]]]
  if (!sim[1] %in% used) ready(sim)
  used <- union(used, sim[1])
  # A home and a copy of the vault for this picture alone: a tap may write.
  here <- file.path(scratch, paste0(shot[[1]], "-", shot[[2]]))
  dir.create(file.path(here, "home"), recursive = TRUE)
  file.copy(vault, here, recursive = TRUE)
  Sys.setenv(ONYX_HOME = file.path(here, "home"), ONYX_VAULT = file.path(here, "Notebook"), ONYX_NOTE = file.path(here, "Notebook", shot[[4]]), ONYX_APPEARANCE = shot[[3]], ONYX_LANG = "en", ONYX_GO = shot[[5]])
  if (length(shot) > 5) Sys.setenv(ONYX_SETTINGS = shot[[6]]) else Sys.unsetenv("ONYX_SETTINGS")
  simctl("ui", sim[1], "appearance", shot[[3]])
  if (system2("scripts/ios-app.sh", shQuote(sim[1]), stdout = FALSE) != 0) stop("store-shots: the app did not start in ", sim[1])
  # The steps each land half a second or more after the one before.
  Sys.sleep(4 + lengths(strsplit(shot[[5]], ";")) * 0.6)
  out <- file.path("dist/store", shot[[1]], paste0(shot[[2]], ".png"))
  dir.create(dirname(out), recursive = TRUE, showWarnings = FALSE)
  if (simctl("io", sim[1], "screenshot", out) != 0) stop("store-shots: no picture of ", sim[1])
  # The store takes no picture with transparency in it, and simctl's has the channel.
  png::writePNG(png::readPNG(out)[, , 1:3], out)
  cat(out, "\n")
}
for (name in used) simctl("shutdown", name)
unlink(scratch, recursive = TRUE)
