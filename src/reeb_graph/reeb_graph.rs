use std::{collections::HashMap, fmt::Display, fs::OpenOptions, path::Path, io::{BufWriter, Write}, mem::swap};

use petgraph::{prelude::{StableDiGraph, NodeIndex, EdgeIndex}, dot::Dot, visit::EdgeRef};
use crate::mesh::traits::{TopologicalMesh, Mesh, VertexProperties, Marker, MeshMarker};

use super::ordered_triangle::{OrderedTriangle, ReebValue, OrderedEdge, ReebFunction, OrderedVertex};

type ArcIdx = EdgeIndex<usize>;

pub struct ArcData<TMesh: Mesh> {
    edges: Vec<(TMesh::EdgeDescriptor, Option<ArcIdx>)>
}

impl<TMesh: Mesh> ArcData<TMesh> {
    fn new(edge: TMesh::EdgeDescriptor) -> Self { 
        return Self { edges: vec![(edge, None)] };
    }

    #[inline]
    fn next_arc_mapped_to_edge(&self, edge: &TMesh::EdgeDescriptor) -> Option<ArcIdx> {
        return self.edges.iter().find(|(e, _)| e == edge).and_then(|(_, next)| *next);
    }
}

impl<TMesh: Mesh> Display for ArcData<TMesh> {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (edge, next_arc) in self.edges.iter() {
            let next = if next_arc.is_some() { next_arc.unwrap().index().to_string() } else { String::from("N") };
            write!(f, "{}:{} ", edge, next)?;
        }

        return Ok(());
    }
}

pub struct NodeData<TMesh: Mesh> {
    vertex: OrderedVertex<TMesh>,
    reeb_value: ReebValue<TMesh::ScalarType>
}

impl<TMesh: Mesh> NodeData<TMesh> {
    fn new(vertex: OrderedVertex<TMesh>, reeb_value: TMesh::ScalarType) -> Self { 
        return Self { vertex, reeb_value: ReebValue::new(reeb_value) };
    }
}

impl<TMesh: Mesh> Display for NodeData<TMesh> {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return write!(f, "{}:{}", self.vertex.vertex(), self.reeb_value.value());
    }
}

pub struct ReebGraph<TMesh: TopologicalMesh> {
    graph: StableDiGraph<NodeData<TMesh>, ArcData<TMesh>, usize>,
    reeb_func: ReebFunction<TMesh>,
    vertex_node_map: HashMap<TMesh::VertexDescriptor, NodeIndex<usize>>,
    edge_highest_arc_map: HashMap<TMesh::EdgeDescriptor, EdgeIndex<usize>>
}

impl<'a, TMesh: TopologicalMesh + VertexProperties + MeshMarker> ReebGraph<TMesh> {
    pub fn new() -> Self {
        return Self {
            reeb_func: |m, v| m.vertex_position(v).x,
            graph: StableDiGraph::default(),
            vertex_node_map: HashMap::new(),
            edge_highest_arc_map: HashMap::new()
        };
    }

    pub fn scalars(mut self, f: ReebFunction<TMesh>) -> Self {
        self.reeb_func = f;
        return self;
    }

    pub fn build(mut self, mesh: &TMesh) -> StableDiGraph<NodeData<TMesh>, ArcData<TMesh>, usize> {
        let mut reeb_values = mesh.create_vertex_properties_map();

        for vertex in mesh.vertices() {
            let reeb_value = (self.reeb_func)(mesh, &vertex);
            reeb_values[vertex] = ReebValue::new(reeb_value);
        }

        let vertex_order = OrderedVertex::order_mesh_vertices(mesh, &reeb_values);

        for vertex in mesh.vertices() {
            let reeb_value = reeb_values[vertex];
            let ordered_vertex = OrderedVertex::new(vertex, reeb_value, vertex_order[vertex]);
            let weight = NodeData::new(ordered_vertex, reeb_value.value());
            let node = self.graph.add_node(weight);
            self.vertex_node_map.insert(vertex, node);
        }

        let mut marker = mesh.marker();
        for face in mesh.faces() {
             let ordered_face = OrderedTriangle::from_face(&face, mesh, &reeb_values, &vertex_order);

            let a1 = self.create_arc(ordered_face.e1(), &mut marker);
            let a2 = self.create_arc(ordered_face.e2(), &mut marker);
            let mut a3 = self.create_arc(ordered_face.e3(), &mut marker);

            // self.save("bg.dot");

            /*a3 =*/ self.glue(a2, a3, *ordered_face.e2().edge(), *ordered_face.e3().edge());
            
            // self.save("g1.dot");

            a3 = self.edge_highest_arc_map[ordered_face.e3().edge()];
            self.glue(a1, a3, *ordered_face.e1().edge(), *ordered_face.e3().edge());

            // self.save("g2.dot");

        }

        return self.graph;
    }

    fn create_arc(&mut self, ordered_edge: &OrderedEdge<TMesh>, marker: &mut TMesh::Marker) -> EdgeIndex<usize> {
        if marker.is_edge_marked(ordered_edge.edge()) {
            return self.edge_highest_arc_map[ordered_edge.edge()];
        }
        
        marker.mark_edge(ordered_edge.edge(), true);

        let n_start = self.vertex_node_map[ordered_edge.start().vertex()];
        let n_end = self.vertex_node_map[ordered_edge.end().vertex()];
        let weight = ArcData::new(*ordered_edge.edge());

        let arc = self.graph.add_edge(n_start, n_end, weight);
        self.edge_highest_arc_map.insert(*ordered_edge.edge(), arc);

        return arc;
    }
    
    fn glue(&mut self, mut a1: ArcIdx, mut a2: ArcIdx, mut e1: TMesh::EdgeDescriptor, mut e2: TMesh::EdgeDescriptor) {
        let mut a1_next = Some(a1);
        let mut a2_next = Some(a2);

        loop {
            if a1_next.is_none() || a2_next.is_none() {
                break;
            }

            a1 = a1_next.unwrap();
            a2 = a2_next.unwrap();

            (a1, a2) = self.peek(a1, a2, &mut e1, &mut e2);

            let a1_start = self.bottom(a1);
            let a2_start = self.bottom(a2);

            if a1_start.vertex < a2_start.vertex {
                (a2_next, a1_next) = self.merge(a2, a1, &e2, &e1);
            } else {
                (a1_next, a2_next) = self.merge(a1, a2, &e1, &e2);
            }
        }
    }

    #[inline]
    fn bottom(&self, arc: ArcIdx) -> &NodeData<TMesh> {
        let (start, _) = self.graph.edge_endpoints(arc).unwrap();
        return self.graph.node_weight(start).unwrap();
    }
    
    #[inline]
    fn top(&self, arc: ArcIdx) -> &NodeData<TMesh> {
        let (_, end) = self.graph.edge_endpoints(arc).unwrap();
        return self.graph.node_weight(end).unwrap();
    }

        
    fn peek(&self, mut upper: ArcIdx, mut lower: ArcIdx, upper_edge: &mut TMesh::EdgeDescriptor, lower_edge: &mut TMesh::EdgeDescriptor) -> (ArcIdx, ArcIdx) {
        if self.top(upper).vertex < self.top(lower).vertex { 
            swap(&mut upper, &mut lower);
            swap(upper_edge, lower_edge);
        }

        let (_, mut upper_top) = self.graph.edge_endpoints(upper).unwrap();
        let (_, lower_top) = self.graph.edge_endpoints(lower).unwrap();

        while upper_top != lower_top {
            let arc = &self.graph[upper];
            upper = arc.next_arc_mapped_to_edge(&upper_edge).unwrap();
            (_, upper_top) = self.graph.edge_endpoints(upper).unwrap();
        }

        return (upper, lower);
    }

    ///
    /// Merge two arcs in reeb graph
    /// 1. Remove `target` arc.
    /// 2. Add arc from `target` bottom to `source` bottom
    /// 
    fn merge(&mut self, source: ArcIdx, target: ArcIdx, source_edge: &TMesh::EdgeDescriptor, target_edge: &TMesh::EdgeDescriptor) -> (Option<ArcIdx>, Option<ArcIdx>) {
        if source == target {
            return (
                self.graph[source].next_arc_mapped_to_edge(source_edge),
                self.graph[target].next_arc_mapped_to_edge(target_edge) 
            );
        }

        let (target_bottom, target_top) = self.graph.edge_endpoints(target).unwrap();
        let (src_bottom, src_top) = self.graph.edge_endpoints(source).unwrap();

        // while target_top != src_top {
        //     source = self.graph[source].next_arc_mapped_to_edge(source_edge).unwrap();
        //     (src_bottom, src_top) = self.graph.edge_endpoints(source).unwrap();
        // }
        debug_assert!(target_top == src_top);

        // Remove references to removed arc
        let outgoing_arcs = self.graph.edges_directed(target_top, petgraph::Direction::Outgoing)
            .map(|arc_ref| ArcIdx::new(arc_ref.id().index()))
            .collect::<Vec<_>>();
        for arc in outgoing_arcs {
            for edge_data in &mut self.graph[arc].edges {
                if edge_data.1.is_some() && edge_data.1.unwrap() == target {
                    edge_data.1 = Some(source);
                }
            }
        }

        let target_arc = self.graph.remove_edge(target).unwrap();

        // Update highest arc for edges mapped to removed arc
        for (edge, _) in &target_arc.edges {
            let highest = self.edge_highest_arc_map[&edge];

            if highest == target {
                self.edge_highest_arc_map.insert(*edge, source);
            }
        }

        let mut target_next = target_arc.next_arc_mapped_to_edge(target_edge);
        let mut target_edges = target_arc.edges.clone();

        if target_bottom != src_bottom {
            let new_arc = Some(self.graph.add_edge(target_bottom, src_bottom, target_arc));
            target_next = new_arc;

            for edge in &mut target_edges {
                edge.1 = new_arc;
            }
        }

        // for (edge, _) in &target_edges {
        //     self.update_highest_arc(*edge, source);
        // }

        let source_arc = &mut self.graph[source];
        source_arc.edges.extend(target_edges);
        let source_next = source_arc.next_arc_mapped_to_edge(source_edge); // Move one line up?

        return (source_next, target_next);
    }

    fn _save(&self, filename: &str) {
        let file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open( Path::new(filename))
            .unwrap();
        let mut writer = BufWriter::new(file);
        let dot = Dot::with_attr_getters(
            &self.graph, 
            &[],
            &|_, arc| format!("headlabel=\"{}\"", arc.id().index()),
            &|_, (_, _)| String::new()
        );
        writer.write(format!("{}", dot).replace("}", "rankdir=\"BT\"}").as_bytes()).expect("");
    }
}

#[cfg(test)]
mod tests {
    use nalgebra::Point3;

    use crate::mesh::{corner_table::prelude::CornerTableF, traits::Mesh};

    use super::ReebGraph;

    #[test]
    fn test_reeb_graph1() {
        let mesh: CornerTableF = CornerTableF::from_vertices_and_indices(
            &vec![
                Point3::<f32>::new(0.0,  0.0, 0.0),
                Point3::<f32>::new(2.0,  3.0, 0.0),
                Point3::<f32>::new(-1.0,  4.0, 0.0),
                Point3::<f32>::new(-2.0,  2.0, 0.0),
            ], 
            &vec![
                0, 1, 2,
                0, 2, 3,
            ]  
        );

        let _ = ReebGraph::new().scalars(|m: &CornerTableF, v| m.vertex_position(v).y).build(&mesh);
    }

    #[test]
    fn test_reeb_graph2() {
        let mesh: CornerTableF = CornerTableF::from_vertices_and_indices(
            &vec![
                Point3::<f32>::new(0.0,  0.0, 0.0),
                Point3::<f32>::new(2.0,  3.0, 0.0),
                Point3::<f32>::new(-1.0,  4.0, 0.0),
                Point3::<f32>::new(-2.0,  2.0, 0.0),
            ], 
            &vec![
                0, 2, 3,
                0, 1, 2,
            ]  
        );

        let _ = ReebGraph::new().scalars(|m: &CornerTableF, v| m.vertex_position(v).y).build(&mesh);
    }
}
