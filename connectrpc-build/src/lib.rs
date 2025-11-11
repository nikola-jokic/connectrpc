use proc_macro2::TokenStream;
use prost_build::Module;
use quote::format_ident;
use quote::quote;
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use syn::parse_quote;

#[derive(Debug, Clone, Default, Copy)]
pub struct GeneratorFeatures {
    reqwest: Option<GeneratorReqwestFeatures>,
    axum: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct GeneratorReqwestFeatures {
    pub proto: bool,
    pub json: bool,
}

impl GeneratorFeatures {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reqwest(mut self, options: GeneratorReqwestFeatures) -> Self {
        self.reqwest = Some(options);
        self
    }

    pub fn axum(mut self) -> Self {
        self.axum = true;
        self
    }

    pub fn full(mut self) -> Self {
        self.reqwest = Some(GeneratorReqwestFeatures {
            proto: true,
            json: true,
        });
        self.axum = true;
        self
    }
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub includes: Vec<PathBuf>,
    pub inputs: Vec<PathBuf>,
    pub protoc_args: Vec<String>,
    pub protoc_version: String,

    pub features: GeneratorFeatures,
    // Map of protobuf package prefixes to Rust module paths for extern types.
    pub extern_paths: BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            includes: Vec::new(),
            inputs: Vec::new(),
            protoc_args: Vec::new(),
            protoc_version: "31.1".to_string(),

            features: GeneratorFeatures::default().full(),
            extern_paths: {
                let mut m = BTreeMap::new();
                m.insert(".google.protobuf".to_string(), "::pbjson_types".to_string());
                m
            },
        }
    }
}

impl Settings {
    pub fn from_directory_recursive<P>(path: P) -> anyhow::Result<Self>
    where
        P: Into<PathBuf>,
    {
        let path = path.into();
        let mut settings = Self::default();
        settings.includes.push(path.clone());

        // Recursively add all files that end in ".proto" to the inputs.
        let mut dirs = vec![path];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path.clone());
                } else if path.extension().map(|ext| ext == "proto").unwrap_or(false) {
                    settings.inputs.push(path);
                }
            }
        }

        Ok(settings)
    }

    pub fn generate(&self) -> anyhow::Result<BTreeMap<String, String>> {
        let out_dir = env::var("OUT_DIR").unwrap();
        let protoc_path = protoc_fetcher::protoc(&self.protoc_version, Path::new(&out_dir))?;
        unsafe {
            env::set_var("PROTOC", protoc_path);
        }

        // Tell cargo to rerun the build if any of the inputs change.
        for input in &self.inputs {
            println!("cargo:rerun-if-changed={}", input.display());
        }

        let descriptor_path =
            PathBuf::from(env::var("OUT_DIR").unwrap()).join("file_descriptor.bin");

        let mut conf = prost_build::Config::new();

        // Standard prost configuration
        conf.compile_well_known_types();
        conf.file_descriptor_set_path(&descriptor_path);
        conf.default_package_filename("connectrpc.rs");
        conf.service_generator(Box::new(service_generator(self.features)));

        // Apply extern paths
        for (proto_prefix, rust_path) in &self.extern_paths {
            conf.extern_path(proto_prefix, rust_path);
        }

        // Arg configuration
        for arg in &self.protoc_args {
            conf.protoc_arg(arg);
        }

        // Have to load and generate by hand because I don't care about hacks.
        let file_descriptor_set = conf.load_fds(&self.inputs, &self.includes)?;
        let requests = file_descriptor_set
            .file
            .into_iter()
            .map(|descriptor| {
                (
                    Module::from_protobuf_package_name(descriptor.package()),
                    descriptor,
                )
            })
            .collect::<Vec<_>>();

        let mut modules = conf
            .generate(requests)?
            .into_iter()
            .map(|(m, c)| (m.parts().collect::<Vec<_>>().join("."), c))
            .collect::<BTreeMap<_, _>>();

        let descriptor_set = std::fs::read(&descriptor_path)?;

        let mut builder = pbjson_build::Builder::new();
        builder.register_descriptors(&descriptor_set)?;

        for (path, rust_path) in &self.extern_paths {
            builder.extern_path(path, rust_path);
        }
        let writers = builder.generate(&["."], move |_package| Ok(Vec::new()))?;

        for (package, output) in writers {
            modules.entry(package.to_string()).and_modify(|c| {
                c.push('\n');
                c.push_str(&String::from_utf8_lossy(&output));
            });
        }

        Ok(modules)
    }
}

struct Service {
    /// The name of the server trait, as parsed into a Rust identifier.
    rpc_trait_name: syn::Ident,

    /// The fully qualified protobuf name of this Service.
    fqn: String,

    /// The methods that make up this service.
    methods: Vec<Method>,
}

struct Method {
    /// The name of the method, as parsed into a Rust identifier.
    name: syn::Ident,

    /// The name of the method as it appears in the protobuf definition.
    proto_name: String,

    /// The input type of this method.
    input_type: syn::Type,

    /// The output type of this method.
    output_type: syn::Type,

    call_type: CallType,
}

enum CallType {
    Unary,
    ClientStreaming,
    ServerStreaming,
    BidiStreaming,
}

impl Service {
    fn from_prost(s: prost_build::Service) -> Self {
        let fqn = format!("{}.{}", s.package, s.proto_name);
        let rpc_trait_name = format_ident!("{}", &s.name);
        let methods = s
            .methods
            .into_iter()
            .map(|m| Method::from_prost(&s.package, &s.proto_name, m))
            .collect();

        Self {
            rpc_trait_name,
            fqn,
            methods,
        }
    }
}

impl Method {
    fn from_prost(pkg_name: &str, svc_name: &str, m: prost_build::Method) -> Self {
        let as_type = |s| -> syn::Type {
            let Ok(typ) = syn::parse_str::<syn::Type>(s) else {
                panic!(
                    "connectrpc-client-build build failed generated invalid Rust while processing {pkg}.{svc}/{name}). this is a bug in connectrpc-client-build, please file a GitHub issue",
                    pkg = pkg_name,
                    svc = svc_name,
                    name = m.proto_name,
                );
            };
            typ
        };

        let input_type = as_type(&m.input_type);
        let output_type = as_type(&m.output_type);
        let name = format_ident!("{}", m.name);
        let message = m.proto_name;

        Self {
            name,
            proto_name: message,
            input_type,
            output_type,
            call_type: if m.client_streaming && m.server_streaming {
                CallType::BidiStreaming
            } else if m.client_streaming {
                CallType::ClientStreaming
            } else if m.server_streaming {
                CallType::ServerStreaming
            } else {
                CallType::Unary
            },
        }
    }
}

#[derive(Clone, Copy)]
pub struct ServiceGenerator {
    features: GeneratorFeatures,
}

pub fn service_generator(features: GeneratorFeatures) -> ServiceGenerator {
    ServiceGenerator { features }
}

impl prost_build::ServiceGenerator for ServiceGenerator {
    fn generate(&mut self, service: prost_build::Service, buf: &mut String) {
        if self.features.reqwest.is_none() && !self.features.axum {
            return;
        }

        let service = Service::from_prost(service);
        let mut use_items: Vec<syn::ItemUse> = vec![];
        use_items.push(parse_quote! {
            pub use ::connectrpc;
        });

        let mut tokens: Vec<syn::Item> = Vec::new();

        let async_service_trait = format_ident!("{}AsyncService", service.rpc_trait_name);
        tokens.push(generate_async_service_trait(&service, &async_service_trait).into());

        let has_unary = service
            .methods
            .iter()
            .any(|m| matches!(m.call_type, CallType::Unary));
        let has_streaming = service.methods.iter().any(|m| {
            matches!(
                m.call_type,
                CallType::ClientStreaming | CallType::ServerStreaming | CallType::BidiStreaming
            )
        });

        if let Some(reqwest_features) = self.features.reqwest {
            let mut generates = vec![];
            if reqwest_features.proto {
                let codec = format_ident!("Proto");
                let client = format_ident!("{}Reqwest{}Client", service.rpc_trait_name, codec);
                generates.push((client, codec));
            }
            if reqwest_features.json {
                let codec = format_ident!("Json");
                let client = format_ident!("{}Reqwest{}Client", service.rpc_trait_name, codec);
                generates.push((client, codec));
            }
            if has_unary {
                use_items.push(parse_quote! {
                    use ::connectrpc::client::AsyncUnaryClient;
                });
            }
            if has_streaming {
                use_items.push(parse_quote! {
                    use ::connectrpc::client::AsyncStreamingClient;
                })
            }
            for (client, codec) in generates {
                tokens.push(generate_reqwest_client_struct(&service, &client).into());
                tokens.push(generate_reqwest_client_impl(&service, &client, &codec).into());
                tokens.push(
                    generate_reqwest_client_trait_impl(&service, &async_service_trait, &client)
                        .into(),
                );
            }
        }

        if self.features.axum {
            let server_struct = format_ident!("{}AxumServer", service.rpc_trait_name);
            tokens.push(generate_axum_server_struct(&service, &server_struct).into());
            tokens.push(generate_axum_server_impl(&service, &server_struct).into());
        }

        let ast: syn::File = parse_quote! {
            // Auto-generated by connectrpc-client-build. Do not edit.
            #(#use_items)*

            #(#tokens)*

        };

        let code = prettyplease::unparse(&ast);
        buf.push_str(&code);
    }
}

fn generate_async_service_trait(service: &Service, service_trait: &syn::Ident) -> syn::ItemTrait {
    let mut trait_methods: Vec<syn::TraitItemFn> = Vec::with_capacity(service.methods.len());

    for m in &service.methods {
        let name = &m.name;
        let input_type = &m.input_type;
        let output_type = &m.output_type;

        match m.call_type {
            CallType::Unary => {
                trait_methods.push(parse_quote! {
                fn #name(
                    &self,
                    request: ::connectrpc::UnaryRequest<#input_type>
                ) -> impl std::future::Future<Output = ::connectrpc::Result<::connectrpc::UnaryResponse<#output_type>>> + Send + '_;
            });
            }
            CallType::ClientStreaming => {
                trait_methods.push(parse_quote! {
                fn #name(
                    &self,
                    request: ::connectrpc::ClientStreamingRequest<#input_type>
                ) -> impl std::future::Future<Output = ::connectrpc::Result<::connectrpc::ClientStreamingResponse<#output_type>>> + Send + '_;
            });
            }
            CallType::ServerStreaming => {
                trait_methods.push(parse_quote! {
                fn #name(
                    &self,
                    request: ::connectrpc::ServerStreamingRequest<#input_type>
                ) -> impl std::future::Future<Output = ::connectrpc::Result<::connectrpc::ServerStreamingResponse<#output_type>>> + Send + '_;
                });
            }
            CallType::BidiStreaming => {
                panic!("Not supported bidi")
            }
        };
    }

    parse_quote! {
        pub trait #service_trait: Send + Sync {
            #(#trait_methods)*
        }
    }
}

fn generate_reqwest_client_struct(service: &Service, client: &syn::Ident) -> syn::ItemStruct {
    let mut client_fields: Vec<TokenStream> = Vec::with_capacity(service.methods.len());

    for m in &service.methods {
        let name = &m.name;

        client_fields.push(quote! {
            pub #name: ::connectrpc::ReqwestClient,
        });
    }

    parse_quote! {
        #[derive(Clone)]
        pub struct #client {
            #(#client_fields)*
        }
    }
}

fn generate_reqwest_client_impl(
    service: &Service,
    struct_name: &syn::Ident,
    codec_ident: &syn::Ident,
) -> syn::ItemImpl {
    let mut client_inits: Vec<TokenStream> = Vec::with_capacity(service.methods.len());

    for m in &service.methods {
        let name = &m.name;

        client_inits.push(quote! {
            #name: ::connectrpc::ReqwestClient::new(client.clone(), base_uri.clone(), ::connectrpc::codec::Codec::#codec_ident)?,
        });
    }

    parse_quote! {
        impl #struct_name {
            pub fn new(client: ::reqwest::Client, base_uri: ::connectrpc::http::Uri) -> ::connectrpc::Result<Self> {
                Ok(Self {
                    #(#client_inits)*
                })
            }
        }
    }
}

fn generate_reqwest_client_trait_impl(
    service: &Service,
    trait_name: &syn::Ident,
    struct_name: &syn::Ident,
) -> syn::ItemImpl {
    let mut client_methods: Vec<syn::ImplItemFn> = Vec::with_capacity(service.methods.len());
    for m in &service.methods {
        let name = &m.name;
        let input_type = &m.input_type;
        let output_type = &m.output_type;
        let path = format!("/{}/{}", service.fqn, m.proto_name);

        match m.call_type {
            CallType::Unary => {
                client_methods.push(parse_quote! {
                    async fn #name(
                        &self,
                        request: ::connectrpc::UnaryRequest<#input_type>
                    ) -> ::connectrpc::Result<::connectrpc::UnaryResponse<#output_type>> {
                        self.#name.call_unary(#path, request).await
                    }
                });
            }
            CallType::ClientStreaming => {
                client_methods.push(parse_quote! {
                    async fn #name(
                        &self,
                        request: ::connectrpc::ClientStreamingRequest<#input_type>
                    ) -> ::connectrpc::Result<::connectrpc::ClientStreamingResponse<#output_type>> {
                        self.#name.call_client_streaming(#path, request).await
                    }
                });
            }
            CallType::ServerStreaming => {
                client_methods.push(parse_quote! {
                    async fn #name(
                        &self,
                        request: ::connectrpc::ServerStreamingRequest<#input_type>
                    ) -> ::connectrpc::Result<::connectrpc::ServerStreamingResponse<#output_type>> {
                        self.#name.call_server_streaming(#path, request).await
                    }
                });
            }
            _ => {
                panic!("Only unary methods are supported in reqwest client");
            }
        }
    }

    parse_quote! {
        impl #trait_name for #struct_name {
            #(#client_methods)*
        }
    }
}

fn generate_axum_server_struct(service: &Service, struct_name: &syn::Ident) -> syn::ItemStruct {
    let mut handlers = Vec::with_capacity(service.methods.len());
    for i in 1..=service.methods.len() {
        handlers.push(format_ident!("H{i}"));
    }

    let mut handler_constraints = Vec::with_capacity(service.methods.len());
    let mut fields = Vec::with_capacity(service.methods.len());

    for (i, method) in service.methods.iter().enumerate() {
        let name = &method.name;
        let input_type = &method.input_type;
        let output_type = &method.output_type;

        let handler = &handlers[i];

        match method.call_type {
            CallType::Unary => {
                handler_constraints.push(quote! {
                    #handler: ::connectrpc::server::axum::RpcUnaryHandler<#input_type, #output_type, S>,
                });
            }
            CallType::ClientStreaming => {
                handler_constraints.push(quote! {
                    #handler: ::connectrpc::server::axum::RpcClientStreamingHandler<#input_type, #output_type, S>,
                });
            }
            CallType::ServerStreaming => {
                handler_constraints.push(quote! {
                    #handler: ::connectrpc::server::axum::RpcServerStreamingHandler<#input_type, #output_type, S>,
                });
            }
            _ => {
                panic!("Only unary methods are supported in axum server");
            }
        }

        fields.push(quote! {
            pub #name: #handler,
        });
    }

    parse_quote! {
        pub struct #struct_name<S, #(#handlers,)*>
        where
            S: Send + Sync + Clone + 'static,
            #(#handler_constraints)*
        {
            pub state: S,
            #(#fields)*
        }
    }
}

fn generate_axum_server_impl(service: &Service, struct_name: &syn::Ident) -> syn::ItemImpl {
    let mut route_inits = Vec::with_capacity(service.methods.len());

    let mut handlers = Vec::with_capacity(service.methods.len());
    for i in 1..=service.methods.len() {
        handlers.push(format_ident!("H{i}"));
    }

    let mut handler_constraints = Vec::with_capacity(service.methods.len());
    for (i, method) in service.methods.iter().enumerate() {
        let name = &method.name;
        let path = format!("/{}/{}", service.fqn, method.proto_name);
        let input_type = &method.input_type;
        let output_type = &method.output_type;

        route_inits.push(quote! {
            let #name = self.#name;
            let cs = common_server.clone();
            router = router.route(
                #path,
                ::axum::routing::any(move |::axum::extract::State(state): ::axum::extract::State<S>, req: ::axum::extract::Request| async move {
                    #name.call(req, state, cs).await
                })
            );
        });

        let handler = &handlers[i];
        match method.call_type {
            CallType::Unary => {
                handler_constraints.push(quote! {
                    #handler: ::connectrpc::server::axum::RpcUnaryHandler<#input_type, #output_type, S>,
                });
            }
            CallType::ClientStreaming => {
                handler_constraints.push(quote! {
                    #handler: ::connectrpc::server::axum::RpcClientStreamingHandler<#input_type, #output_type, S>,
                });
            }
            CallType::ServerStreaming => {
                handler_constraints.push(quote! {
                    #handler: ::connectrpc::server::axum::RpcServerStreamingHandler<#input_type, #output_type, S>,
                });
            }
            _ => {
                panic!("Only unary methods are supported in axum server");
            }
        }
    }

    parse_quote! {
        impl<S, #(#handlers,)*> #struct_name<S, #(#handlers,)*>
        where
            S: Send + Sync + Clone + 'static,
            #(#handler_constraints)*
        {
            pub fn into_router(
                self,
            ) -> ::axum::Router {
                let mut router = ::axum::Router::new();
                let common_server = ::connectrpc::server::CommonServer::new();
                #(#route_inits)*
                router.with_state(self.state)
            }
        }
    }
}
