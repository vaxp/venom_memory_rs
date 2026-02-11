# Venom Audio Daemon Makefile (VenomMemory IPC Edition)

CC = gcc
CFLAGS = -Wall -Wextra -O2 -I./include -I../include $(shell pkg-config --cflags glib-2.0 libpulse)
LDFLAGS = $(shell pkg-config --libs glib-2.0 libpulse) -L../target/release -lvenom_memory -Wl,-rpath,../target/release -lpthread

SRC_DIR = src
BUILD_DIR = build
TARGET = venom_audio

# Exclude dbus_service.c - we're using venom_ipc.c instead
SOURCES = $(SRC_DIR)/main.c $(SRC_DIR)/audio.c $(SRC_DIR)/venom_ipc.c
OBJECTS = $(BUILD_DIR)/main.o $(BUILD_DIR)/audio.o $(BUILD_DIR)/venom_ipc.o

.PHONY: all clean install build-lib

all: build-lib $(TARGET)

# Build VenomMemory library first
build-lib:
	@echo "🦀 Building VenomMemory Rust library..."
	@cd .. && cargo build --release

$(TARGET): $(OBJECTS)
	@echo "🔗 Linking $(TARGET)..."
	@$(CC) $(OBJECTS) -o $@ $(LDFLAGS)
	@echo "✅ Build complete: $(TARGET)"

$(BUILD_DIR)/%.o: $(SRC_DIR)/%.c | $(BUILD_DIR)
	@echo "📦 Compiling $<..."
	@$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR):
	@mkdir -p $(BUILD_DIR)

clean:
	@echo "🧹 Cleaning..."
	@rm -rf $(BUILD_DIR) $(TARGET)
	@echo "✅ Clean complete"

install: $(TARGET)
	@echo "📦 Installing..."
	@sudo cp $(TARGET) /usr/local/bin/
	@sudo cp ../target/release/libvenom_memory.so /usr/local/lib/
	@sudo ldconfig
	@echo "✅ Installed to /usr/local/bin/$(TARGET)"

run: $(TARGET)
	@echo "🚀 Running $(TARGET)..."
	@LD_LIBRARY_PATH=../target/release ./$(TARGET)
