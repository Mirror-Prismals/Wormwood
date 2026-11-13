    use super::Host;
    use crate::{audio::*,base_system::layer_five::glasses_system::{self,Carry}};
    use serde_json::{Value,json};
    fn dispatch(host:&mut Host,hook:&str,input:&Value)->Result<Value,String>{
        host.stack.push((0,format!("GlassesSystem.{hook}")));
        let result=glasses_system::dispatch(host,hook,input);host.stack.pop();result
    }
    fn input(f:bool,b:bool,e:bool)->Value {json!({"view":{"position":[-0.95,8.,-49.],"forward":[0.,0.,1.],"up":[0.,1.,0.]},"window":{"flight":{"focused":true,"glasses":{"interact":f,"equip":b,"open":e}}}})}
    #[test]fn glasses_both_pairs_share_wear_open_edges_and_focus_guards(){
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));let out=std::env::temp_dir().join(format!("mirror-glasses-{}",std::process::id()));
        let mut host=Host::load_paths(root,&out).unwrap();host.invoke("TheatreSystem.Configure",Value::Null).unwrap();dispatch(&mut host,"Configure",&Value::Null).unwrap();
        for i in 0..2 {
            host.glasses.pairs[i].location=Carry::Held;
            let grip=dispatch(&mut host,"Grip",&input(false,false,false)["view"]).unwrap();assert_eq!(grip["active"],true);
            let worn=dispatch(&mut host,"Input",&input(false,true,false)).unwrap();assert_eq!(worn["worn"],true);assert_eq!(worn["held"],false);
            assert_eq!(dispatch(&mut host,"Grip",&input(false,false,false)["view"]).unwrap()["active"],false);
            let mut blocked=input(false,false,true);blocked["blocked"]=json!(true);
            assert_eq!(dispatch(&mut host,"Input",&blocked).unwrap()["open"],false);
            // Focus recovery with E still held must not synthesize a new edge.
            assert_eq!(dispatch(&mut host,"Input",&input(false,false,true)).unwrap()["open"],false);
            dispatch(&mut host,"Input",&input(false,false,false)).unwrap();
            assert_eq!(dispatch(&mut host,"Input",&input(false,false,true)).unwrap()["open"],true);
            assert_eq!(dispatch(&mut host,"Input",&input(false,false,true)).unwrap()["open"],false);
            dispatch(&mut host,"Input",&input(false,false,false)).unwrap();
            let mut focused=input(false,true,false);focused["phone_focused"]=json!(true);
            assert_eq!(dispatch(&mut host,"Input",&focused).unwrap()["worn"],true);
            dispatch(&mut host,"Input",&input(false,false,false)).unwrap();
            assert_eq!(dispatch(&mut host,"Input",&input(false,true,false)).unwrap()["held"],true);
            host.glasses.pairs[i].location=Carry::Placed;dispatch(&mut host,"Input",&input(false,false,false)).unwrap();
        }
        dispatch(&mut host,"Close",&Value::Null).unwrap();assert!(host.glasses.pairs.is_empty());
    }
    #[test]fn glasses_exports_pick_at_authored_scale_and_render_in_shared_depth(){
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));let out=std::env::temp_dir().join(format!("mirror-glasses-model-{}",std::process::id()));
        let mut host=Host::load_paths(root,&out).unwrap();host.invoke("TheatreSystem.Configure",Value::Null).unwrap();dispatch(&mut host,"Configure",&Value::Null).unwrap();
        for p in &host.glasses.pairs {
            assert!(length(sub(p.pose.position,host.theatre.world([0.,7.5,-47.2])))<1.5);
            let mesh=&host.glasses_render.models[p.kind];let eye=add(p.pose.position,[0.036,0.,1.]);let ray=[0.,0.,-1.];assert!(glasses_system::hit(mesh,p.pose,eye,ray).is_some(),"{} must pick at its actual world position",p.kind);
            let eye=add(p.pose.position,[0.,0.15,0.8]);let v=json!({"position":eye,"yaw":0.,"pitch":-0.15,"aspect":1.5,"right":[1.,0.,0.],"up":[0.,1.,0.],"forward":[0.,0.,-1.]});
            let rendered=crate::base_system::layer_four::glasses_render_system::frame(&host,&v).unwrap();assert!(!rendered.is_empty());assert!(rendered.iter().flatten().all(|v|v.is_finite()));
        }
    }
    #[test]fn glasses_pickup_uses_real_theatre_obstruction_and_surface_placement(){
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));let out=std::env::temp_dir().join(format!("mirror-glasses-pick-{}",std::process::id()));
        let mut host=Host::load_paths(root,&out).unwrap();host.invoke("TheatreSystem.Configure",Value::Null).unwrap();host.invoke("CaveSystem.Configure",Value::Null).unwrap();dispatch(&mut host,"Configure",&Value::Null).unwrap();
        for i in 0..2 {
            let target=add(host.glasses.pairs[i].pose.position,[0.036,0.,0.]);
            let mut command=input(true,false,false);command["view"]=json!({"position":add(target,[0.,0.,-1.]),"forward":[0.,0.,1.],"up":[0.,1.,0.],"right":[-1.,0.,0.]});
            let status=dispatch(&mut host,"Input",&command).unwrap();assert_eq!(status["held"],true,"{} pickup: {status}",i);
            command["window"]["flight"]["glasses"]["interact"]=json!(false);dispatch(&mut host,"Input",&command).unwrap();
            command["view"]=json!({"position":host.theatre.world([-1.1,8.3,-47.4]),"forward":[0.,-1.,0.],"up":[0.,1.,0.],"right":[1.,0.,0.]});
            command["window"]["flight"]["glasses"]["interact"]=json!(true);
            assert_eq!(dispatch(&mut host,"Input",&command).unwrap()["held"],false);
            assert!(host.glasses.pairs[i].pose.position[1]>host.theatre.world([0.,7.45,0.])[1]);
            command["window"]["flight"]["glasses"]["interact"]=json!(false);dispatch(&mut host,"Input",&command).unwrap();
        }
    }
