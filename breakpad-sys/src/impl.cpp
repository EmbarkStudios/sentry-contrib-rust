#include <stdint.h>
#include <string.h>

#include "exception_handler.h"

#ifdef TARGET_OS_WINDOWS
    #define CHAR_TYPE uint16_t
#else
    #define CHAR_TYPE uint8_t
#endif

// Callback invoked when a minidump occurs. Returns the path + length of the
// minidump file, along with the callback context.
typedef void (*dump_callback)(const CHAR_TYPE*, size_t, void*);

struct BreakpadContext {
    dump_callback callback;
    void* callback_ctx;
};

struct ExceptionHandler {
    BreakpadContext* bp_ctx;
    google_breakpad::ExceptionHandler* handler;
};

extern "C" {
    ExceptionHandler* attach_exception_handler(
        const CHAR_TYPE* path,
        size_t path_len,
        dump_callback callback,
        void* callback_ctx
    ) {
        auto* bp_ctx = new BreakpadContext;
        bp_ctx->callback = callback;
        bp_ctx->callback_ctx = callback_ctx;

        #ifdef TARGET_OS_WINDOWS
            std::wstring dump_path(reinterpret_cast<const wchar_t*>(path), path_len / (sizeof(wchar_t) / sizeof(uint8_t)));

            auto crash_callback = [](
                const wchar_t* breakpad_dump_path,
                const wchar_t* minidump_id,
                void* context,
                EXCEPTION_POINTERS*,
                MDRawAssertionInfo*,
                bool succeeded
            ) -> bool {
                auto* ctx = (BreakpadContext*)context;

                // We have to construct the full path to the minidump file ourselves
                google_breakpad::wstring dump_path(breakpad_dump_path);
                dump_path.push_back('/');
                dump_path.append(minidump_id);
                dump_path.append(L".dmp");

                ctx->callback(
                    reinterpret_cast<const CHAR_TYPE*>(dump_path.data()),
                    dump_path.size() * (sizeof(wchar_t) / sizeof(uint8_t)),
                    ctx->callback_ctx
                );

                return succeeded;
            };

            auto* handler = new google_breakpad::ExceptionHandler(
                dump_path, // Directory to store the minidump in
                nullptr, // Minidump write filter, might be used later
                crash_callback, // Callback invoked after the minidump has been written
                bp_ctx, // Callback context
                google_breakpad::ExceptionHandler::HANDLER_EXCEPTION // Write minidumps when a structured exception occurs
            );
        #elif defined(TARGET_OS_MAC)
            std::string dump_path(reinterpret_cast<const char*>(path), path_len);

            auto crash_callback = [](
                const char* dump_dir,
                const char* minidump_id,
                void* context,
                bool succeeded
            ) -> bool {
                auto* ctx = (BreakpadContext*)context;

                google_breakpad::string dump_path(dump_dir);
                dump_path.push_back('/');
                dump_path.append(minidump_id);
                dump_path.append(".dmp");

                ctx->callback(
                    reinterpret_cast<const CHAR_TYPE*>(dump_path.data()),
                    dump_path.size(),
                    ctx->callback_ctx
                );

                return succeeded;
            };

            auto* handler = new google_breakpad::ExceptionHandler(
                dump_path, // Directory to store the minidump in
                nullptr, // Minidump write filter, might be used later
                crash_callback, // Callback invoked after the minidump has been written
                bp_ctx, // Callback context
                true, // Actually write minidumps when unhandled signals occur
                nullptr, // Don't start a separate process, handle crashes in the same process
            );
        #elif defined(TARGET_OS_LINUX)
            std::string dump_path(reinterpret_cast<const char*>(path), path_len);
            google_breakpad::MinidumpDescriptor descriptor(dump_path);

            auto crash_callback = [](
                const google_breakpad::MinidumpDescriptor& descriptor,
                void* context,
                bool succeeded
            ) -> bool {
                auto* ctx = (BreakpadContext*)context;

                auto* dump_path = descriptor.path();

                ctx->callback(
                    reinterpret_cast<const CHAR_TYPE*>(dump_path),
                    strlen(dump_path),
                    ctx->callback_ctx
                );

                return succeeded;
            };

            auto* handler = new google_breakpad::ExceptionHandler(
                descriptor, // Decides where to place the minidump file
                nullptr, // Minidump write filter, might be used later
                crash_callback, // Callback invoked after the minidump has been written
                bp_ctx, // Callback context
                true, // Actually write minidumps when unhandled signals occur
                -1 // Don't start a separate process, handle crashes in the same process
            );
        #else
            #error "Unknown target platform"
        #endif

        auto* exc_handler = new ExceptionHandler;
        exc_handler->bp_ctx = bp_ctx;
        exc_handler->handler = handler;

        return exc_handler;
    }

    void detach_exception_handler(ExceptionHandler* handler) {
        delete handler->bp_ctx;
        delete handler->handler;
        delete handler;
    }
}
