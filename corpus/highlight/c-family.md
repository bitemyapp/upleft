# C, C++, Objective-C

```c
#include <stdio.h>
#define MAX_LEN 256
#if defined(__APPLE__) && !defined(NDEBUG)
#endif

typedef struct node { int value; struct node *next; } node_t;
static const char *NAMES[] = { "a", "b\n", "c\"" };

int main(int argc, char **argv) {
    size_t n = sizeof(node_t) + 0x1Fu + 07 + 1.5e3f + 'c' + '\0';
    unsigned long long big = 18446744073709551615ULL;
    for (int i = 0; i < argc; i++) { printf("%s\n", argv[i]); }
    // line comment
    /* block comment */
    return n > MAX_LEN ? EXIT_FAILURE : 0;
}
```

```h
_Bool flag = true; _Atomic int counter; void *ptr = NULL;
```

```cpp
#include <vector>
#include <string_view>
namespace app {
template <typename T, std::size_t N = 4>
class Buffer final : public Base {
public:
    constexpr explicit Buffer(std::string_view name) noexcept : name_(name) {}
    auto size() const -> std::size_t { return data_.size(); }
    virtual ~Buffer() = default;
private:
    std::vector<T> data_;
    std::unique_ptr<int> owned = nullptr;
};
}
auto raw = R"delim(a raw "string" with )" inside)delim";
auto raw2 = R"(simple)";
auto bad = R"tag(unterminated
co_await task; static_cast<int>(x); this->value = true;
```

```c++
std::cout << "c++ alias" << std::endl;
```

```objc
#import <Foundation/Foundation.h>
@interface Widget : NSObject <NSCopying>
@property (nonatomic, strong, nullable) NSString *title;
- (instancetype)initWithTitle:(NSString *)title;
@end
@implementation Widget
- (instancetype)initWithTitle:(NSString *)title {
    if ((self = [super init])) { _title = [title copy]; }
    NSLog(@"Created %@ with %d", self, YES);
    BOOL ok = NO; id obj = nil; SEL sel = @selector(copy);
    return self;
}
@end
```

```objective-c
NSArray *items = @[@"a", @"b"]; NSDictionary *map = @{@"k": @1};
```
