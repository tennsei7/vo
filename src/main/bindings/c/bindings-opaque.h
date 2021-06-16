/*
 * The Shadow Simulator
 * See LICENSE for licensing information
 */
// clang-format off


#ifndef main_opaque_bindings_h
#define main_opaque_bindings_h

/* Warning, this file is autogenerated by cbindgen. Don't modify this manually. */

typedef enum InterposeMethod {
  // Attach to child using ptrace and use it to interpose syscalls etc.
  INTERPOSE_METHOD_PTRACE,
  // Use LD_PRELOAD to load a library that implements the libC interface which will
  // route syscalls to Shadow.
  INTERPOSE_METHOD_PRELOAD,
  // Use both PRELOAD and PTRACE based interposition.
  INTERPOSE_METHOD_HYBRID,
} InterposeMethod;

typedef enum IpcMethod {
  // Unix-domain socket
  IPC_METHOD_SOCKET,
  // Semaphore + shared memory
  IPC_METHOD_SEMAPHORE,
} IpcMethod;

typedef enum QDiscMode {
  Q_DISC_MODE_FIFO,
  Q_DISC_MODE_ROUND_ROBIN,
} QDiscMode;

// Memory allocated by Shadow, in a remote address space.
typedef struct AllocdMem_u8 AllocdMem_u8;

// A queue of byte chunks.
typedef struct ByteQueue ByteQueue;

// Run real applications over simulated networks.
typedef struct CliOptions CliOptions;

typedef struct CompatDescriptor CompatDescriptor;

// Options contained in a configuration file.
typedef struct ConfigFileOptions ConfigFileOptions;

// Shadow configuration options after processing command-line and configuration file options.
typedef struct ConfigOptions ConfigOptions;

// The main counter object that maps individual keys to count values.
typedef struct Counter Counter;

// Table of (file) descriptors. Typically owned by a Process.
typedef struct DescriptorTable DescriptorTable;

typedef struct HostOptions HostOptions;

// A set of `n` logical processors
typedef struct LogicalProcessors LogicalProcessors;

// Provides accessors for reading and writing another process's memory.
// When in use, any operation that touches that process's memory must go
// through the MemoryManager to ensure soundness. See MemoryManager::new.
typedef struct MemoryManager MemoryManager;

// An opaque type used when passing `*const AtomicRefCell<File>` to C.
typedef struct PosixFileArc PosixFileArc;

// A mutable reference to a slice of plugin memory. Implements DerefMut<[T]>,
// allowing, e.g.:
//
// let tpp = TypedPluginPtr::<u32>::new(ptr, 10);
// let pmr = memory_manager.memory_ref_mut(ptr);
// assert_eq!(pmr.len(), 10);
// pmr[5] = 100;
//
// The object must be disposed of by calling `flush` or `noflush`.  Dropping
// the object without doing so will result in a panic.
typedef struct ProcessMemoryRefMut_u8 ProcessMemoryRefMut_u8;

// An immutable reference to a slice of plugin memory. Implements Deref<[T]>,
// allowing, e.g.:
//
// let tpp = TypedPluginPtr::<u32>::new(ptr, 10);
// let pmr = memory_manager.memory_ref(ptr);
// assert_eq!(pmr.len(), 10);
// let x = pmr[5];
typedef struct ProcessMemoryRef_u8 ProcessMemoryRef_u8;

typedef struct ProcessOptions ProcessOptions;

#endif /* main_opaque_bindings_h */
