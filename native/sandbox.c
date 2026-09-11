/* Supported code-signing entitlement inspection; no private sandbox profile. */
#include <CoreFoundation/CoreFoundation.h>
#include <Security/SecTask.h>

static int entitlement_enabled(SecTaskRef task, CFStringRef name) {
    CFTypeRef value = SecTaskCopyValueForEntitlement(task, name, NULL);
    if (value == NULL) return 0;
    int enabled = CFGetTypeID(value) == CFBooleanGetTypeID() &&
        CFBooleanGetValue((CFBooleanRef)value);
    CFRelease(value);
    return enabled;
}

int gb_usb_entitlements_check(void) {
    SecTaskRef task = SecTaskCreateFromSelf(NULL);
    if (task == NULL) return 0;
    int enabled = entitlement_enabled(task, CFSTR("com.apple.security.app-sandbox")) &&
        entitlement_enabled(task, CFSTR("com.apple.security.device.usb"));
    CFRelease(task);
    return enabled;
}
