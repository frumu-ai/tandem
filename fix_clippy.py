import re

with open('crates/tandem-memory/src/manager_parts/part01.rs', 'r') as f:
    content = f.read()

content = content.replace(
    'pub async fn search_for_tenant_with_access_filter(',
    '#[allow(clippy::too_many_arguments)]\n    pub async fn search_for_tenant_with_access_filter('
)

with open('crates/tandem-memory/src/manager_parts/part01.rs', 'w') as f:
    f.write(content)
