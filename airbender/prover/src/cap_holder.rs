pub use cs::one_row_compiler::array_to_tokens;
pub use cs::one_row_compiler::slice_to_tokens;

impl<const N: usize> quote::ToTokens for crate::definitions::MerkleTreeCap<N> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        use quote::quote;

        let cap = array_to_tokens(&self.cap.map(|el| array_to_tokens(&el)));
        let n = N;

        let stream = quote! {
            MerkleTreeCap::<#n> {
                cap: #cap
            }
        };

        tokens.extend(stream);
    }
}
